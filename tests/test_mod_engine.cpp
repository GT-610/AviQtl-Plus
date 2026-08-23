#include "mod_engine.hpp"
#include "bounded_file.hpp"
#include "package_manager.hpp"
#include "permission_manager.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QMap>
#include <QRegularExpression>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QTextStream>
#include <algorithm>

using namespace AviQtl::Scripting;

class TestModEngine : public QObject {
    Q_OBJECT

  private slots:
    void pluginFileWatcherWatchesAndClearsPaths();
    void apiPermissionMappingCoversRegisteredOperations();
    void filePluginsUseSyntheticPermissionIdentity();
    void loadsAndDispatchesEachPluginLifecycle();
    void rejectsOversizedAndInvalidPlugins();
};

namespace {
struct ApiPermissionCase {
    const char *apiName;
    AviQtl::Core::PluginPermission permission;
};

const QList<ApiPermissionCase> &apiPermissionCases() {
    using AviQtl::Core::PluginPermission;
    static const QList<ApiPermissionCase> cases = {
        {"transport_play", PluginPermission::TransportControl},
        {"transport_pause", PluginPermission::TransportControl},
        {"transport_toggle", PluginPermission::TransportControl},
        {"transport_seek", PluginPermission::TransportControl},
        {"transport_get_frame", PluginPermission::TransportControl},
        {"transport_is_playing", PluginPermission::TransportControl},
        {"clip_list", PluginPermission::ClipRead},
        {"clip_select", PluginPermission::ClipRead},
        {"clip_create", PluginPermission::ClipModify},
        {"clip_delete", PluginPermission::ClipModify},
        {"clip_update", PluginPermission::ClipModify},
        {"clip_split", PluginPermission::ClipModify},
        {"clip_copy", PluginPermission::ClipboardAccess},
        {"clip_cut", PluginPermission::ClipboardAccess},
        {"clip_paste", PluginPermission::ClipboardAccess},
        {"effect_add", PluginPermission::EffectModify},
        {"effect_remove", PluginPermission::EffectModify},
        {"effect_set_param", PluginPermission::EffectModify},
        {"project_width", PluginPermission::ProjectRead},
        {"project_height", PluginPermission::ProjectRead},
        {"project_fps", PluginPermission::ProjectRead},
        {"project_save", PluginPermission::ProjectSave},
        {"project_load", PluginPermission::ProjectLoad},
        {"scene_create", PluginPermission::SceneManage},
        {"scene_remove", PluginPermission::SceneManage},
        {"scene_switch", PluginPermission::SceneManage},
        {"settings_set", PluginPermission::SettingsWrite},
        {"settings_get", PluginPermission::SettingsRead},
        {"undo", PluginPermission::HistoryControl},
        {"redo", PluginPermission::HistoryControl},
        {"command_begin_group", PluginPermission::HistoryControl},
        {"command_end_group", PluginPermission::HistoryControl},
        {"log", PluginPermission::LogOutput},
    };
    return cases;
}

bool writeTextFile(const QString &path, const QString &contents) {
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) {
        return false;
    }
    QTextStream stream(&file);
    stream << contents;
    return stream.status() == QTextStream::Ok;
}

bool createPlugin(const QString &pluginsPath, const QString &directoryName,
                  const QString &pluginId, const QString &pluginName,
                  bool mutateOnClipChange = false) {
    const QString pluginPath = QDir(pluginsPath).filePath(directoryName);
    if (!QDir().mkpath(pluginPath)) {
        return false;
    }
    const QString manifest = QStringLiteral(R"(
return {
    id = "%1",
    name = "%2",
    version = "1.0.0"
}
)").arg(pluginId, pluginName);
    const QString script = QStringLiteral(R"(
local function record(key, value)
    aviqtl.settings.set(key, tostring(value))
end

local mutateOnClipChange = %1
local clipHookCount = 0

function AviQtlOnLoad()
    record("load", "called")
end

function AviQtlUpdateHook()
    record("update", "called")
end

function AviQtlOnProjectOpen(path)
    record("open", path)
end

function AviQtlOnProjectSave(path)
    record("save", path)
end

function AviQtlOnClipChange()
    clipHookCount = clipHookCount + 1
    record("clip", "called")
    record("clip_count", clipHookCount)
    if mutateOnClipChange and clipHookCount == 1 then
        aviqtl.clip.create("rect", 1, 0)
    end
end

function AviQtlOnUnload()
    record("unload", "called")
end
)").arg(mutateOnClipChange ? QStringLiteral("true") : QStringLiteral("false"));
    return writeTextFile(QDir(pluginPath).filePath(QStringLiteral("manifest.lua")), manifest) &&
           writeTextFile(QDir(pluginPath).filePath(QStringLiteral("main.lua")), script);
}
} // namespace

void TestModEngine::pluginFileWatcherWatchesAndClearsPaths() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    PluginFileWatcher watcher;
    QSignalSpy spy(&watcher, &PluginFileWatcher::directoryChanged);
    QVERIFY(spy.isValid());
    watcher.watchPath(dir.path());

    QFile firstFile(dir.filePath(QStringLiteral("first.lua")));
    QVERIFY(firstFile.open(QIODevice::WriteOnly));
    QCOMPARE(firstFile.write("return {}"), 9);
    firstFile.close();

    QTRY_VERIFY_WITH_TIMEOUT(spy.count() > 0, 5'000);
    QCOMPARE(spy.last().at(0).toString(), dir.path());

    watcher.clearPaths();
    QCoreApplication::processEvents();
    spy.clear();

    QFile secondFile(dir.filePath(QStringLiteral("second.lua")));
    QVERIFY(secondFile.open(QIODevice::WriteOnly));
    QCOMPARE(secondFile.write("return {}"), 9);
    secondFile.close();

    QVERIFY(!spy.wait(200));
    QCOMPARE(spy.count(), 0);
}

void TestModEngine::apiPermissionMappingCoversRegisteredOperations() {
    using AviQtl::Core::PermissionManager;
    using AviQtl::Core::PluginPermission;

    ModEngine &engine = ModEngine::instance();
    PermissionManager &permissions = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.mod_engine.permissions");
    const auto cleanup = qScopeGuard([&]() { permissions.revokeAllPermissions(pluginId); });

    QVERIFY(engine.currentPluginId().isEmpty());
    QVERIFY(engine.checkPermission("direct_internal_call"));
    QTest::ignoreMessage(QtWarningMsg, QRegularExpression(QStringLiteral(".*Unknown API name.*unknown_api.*")));
    QVERIFY(!permissions.hasApiPermission(pluginId, "unknown_api"));

    QMap<int, PluginPermission> permissionsByValue;
    for (const ApiPermissionCase &testCase : apiPermissionCases()) {
        permissionsByValue.insert(static_cast<int>(testCase.permission), testCase.permission);
    }
    QCOMPARE(permissionsByValue.size(), PermissionManager::allPermissionNames().size());

    for (PluginPermission granted : std::as_const(permissionsByValue)) {
        permissions.revokeAllPermissions(pluginId);
        permissions.grantPermission(pluginId, granted);
        for (const ApiPermissionCase &testCase : apiPermissionCases()) {
            QCOMPARE(permissions.hasApiPermission(pluginId, testCase.apiName), testCase.permission == granted);
        }
    }
}

void TestModEngine::filePluginsUseSyntheticPermissionIdentity() {
    using AviQtl::Core::PackageManager;
    using AviQtl::Core::PermissionManager;
    using AviQtl::Core::PluginPermission;
    using AviQtl::Core::SettingsManager;

    ModEngine &engine = ModEngine::instance();
    PermissionManager &permissions = PermissionManager::instance();
    SettingsManager &settings = SettingsManager::instance();
    const QVariantMap originalSettings = settings.settings();
    const QString pluginsPath = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("plugins"));
    const QString fileName = QStringLiteral("synthetic_identity.lua");
    const QString filePath = QDir(pluginsPath).filePath(fileName);
    const QString pluginId = QStringLiteral("file:") + fileName;
    const QString settingKey = QStringLiteral("plugin.%1.file_load").arg(pluginId);

    engine.unloadPlugins();
    permissions.revokeAllPermissions(pluginId);
    QFile::remove(filePath);
    const auto cleanup = qScopeGuard([&]() {
        engine.unloadPlugins();
        permissions.revokeAllPermissions(pluginId);
        settings.setSettings(originalSettings);
        QFile::remove(filePath);
    });

    QVERIFY(QDir().mkpath(pluginsPath));
    QVERIFY(writeTextFile(filePath, QStringLiteral(R"(
function AviQtlOnLoad()
    aviqtl.settings.set("file_load", "called")
end
)")));

    engine.loadPlugins();
    const QList<PluginInfo> infos = engine.pluginInfos();
    QVERIFY(std::any_of(infos.cbegin(), infos.cend(), [&pluginId](const PluginInfo &info) {
        return info.manifest.id == pluginId;
    }));
    const QVariantList installedPackages = PackageManager::instance().getPackagesByType(QStringLiteral("installed"));
    const auto packageIt = std::find_if(installedPackages.cbegin(), installedPackages.cend(), [&pluginId](const QVariant &entry) {
        return entry.toMap().value(QStringLiteral("id")).toString() == pluginId;
    });
    QVERIFY(packageIt != installedPackages.cend());
    QVERIFY(packageIt->toMap().value(QStringLiteral("local_file_plugin")).toBool());

    QTest::ignoreMessage(QtCriticalMsg, QRegularExpression(QStringLiteral(".*Permission denied: settings\\.write.*")));
    engine.onLoad();
    QVERIFY(!settings.settings().contains(settingKey));

    permissions.grantPermission(pluginId, PluginPermission::SettingsWrite);
    engine.onLoad();
    QCOMPARE(settings.value(settingKey).toString(), QStringLiteral("called"));
    QVERIFY(engine.currentPluginId().isEmpty());
}

void TestModEngine::loadsAndDispatchesEachPluginLifecycle() {
    using AviQtl::Core::PermissionManager;
    using AviQtl::Core::PluginPermission;
    using AviQtl::Core::SettingsManager;

    ModEngine &engine = ModEngine::instance();
    SettingsManager &settings = SettingsManager::instance();
    PermissionManager &permissions = PermissionManager::instance();
    const QVariantMap originalSettings = settings.settings();
    const QString pluginsPath = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("plugins"));
    const QStringList pluginDirectories = {QStringLiteral("lifecycle_alpha"), QStringLiteral("lifecycle_beta"), QStringLiteral("lifecycle_invalid")};
    const QStringList pluginIds = {QStringLiteral("test.lifecycle.alpha"), QStringLiteral("test.lifecycle.beta")};

    engine.unloadPlugins();
    for (const QString &directory : pluginDirectories) {
        QDir(QDir(pluginsPath).filePath(directory)).removeRecursively();
    }
    const auto cleanup = qScopeGuard([&]() {
        engine.unloadPlugins();
        engine.registerController(nullptr);
        for (const QString &pluginId : pluginIds) {
            permissions.revokeAllPermissions(pluginId);
        }
        settings.setSettings(originalSettings);
        for (const QString &directory : pluginDirectories) {
            QDir(QDir(pluginsPath).filePath(directory)).removeRecursively();
        }
    });

    QVERIFY(QDir().mkpath(pluginsPath));
    QVERIFY(createPlugin(pluginsPath, pluginDirectories.at(0), pluginIds.at(0), QStringLiteral("Lifecycle Alpha"), true));
    QVERIFY(createPlugin(pluginsPath, pluginDirectories.at(1), pluginIds.at(1), QStringLiteral("Lifecycle Beta")));
    const QString invalidPath = QDir(pluginsPath).filePath(pluginDirectories.at(2));
    QVERIFY(QDir().mkpath(invalidPath));
    QVERIFY(writeTextFile(QDir(invalidPath).filePath(QStringLiteral("main.lua")),
                          QStringLiteral("aviqtl.settings.set('should_not_run', 'yes')")));

    for (const QString &pluginId : pluginIds) {
        permissions.revokeAllPermissions(pluginId);
        permissions.grantPermission(pluginId, PluginPermission::SettingsWrite);
        permissions.grantPermission(pluginId, PluginPermission::ClipModify);
    }

    engine.loadPlugins();
    const QList<PluginManifest> loadedPlugins = engine.loadedPlugins();
    const QList<PluginInfo> pluginInfos = engine.pluginInfos();
    for (const QString &pluginId : pluginIds) {
        QVERIFY(std::any_of(loadedPlugins.cbegin(), loadedPlugins.cend(), [&pluginId](const PluginManifest &plugin) {
            return plugin.id == pluginId;
        }));
        QVERIFY(std::any_of(pluginInfos.cbegin(), pluginInfos.cend(), [&pluginId](const PluginInfo &info) {
            return info.manifest.id == pluginId;
        }));
    }
    QVERIFY(engine.currentPluginId().isEmpty());

    engine.onLoad();
    engine.onUpdate();
    const QString openedPath = QStringLiteral("/tmp/项目 open.aviqtl");
    const QString savedPath = QStringLiteral("/tmp/项目 save.aviqtl");
    engine.onProjectOpen(openedPath);
    engine.onProjectSave(savedPath);

    AviQtl::UI::TimelineController controller;
    engine.registerController(&controller);
    controller.createObject(QStringLiteral("rect"), 0, 0);
    QCOMPARE(controller.clips().size(), 2);

    for (const QString &pluginId : pluginIds) {
        const QString prefix = QStringLiteral("plugin.%1.").arg(pluginId);
        QCOMPARE(settings.value(prefix + QStringLiteral("load")).toString(), QStringLiteral("called"));
        QCOMPARE(settings.value(prefix + QStringLiteral("update")).toString(), QStringLiteral("called"));
        QCOMPARE(settings.value(prefix + QStringLiteral("open")).toString(), openedPath);
        QCOMPARE(settings.value(prefix + QStringLiteral("save")).toString(), savedPath);
        QCOMPARE(settings.value(prefix + QStringLiteral("clip")).toString(), QStringLiteral("called"));
        QCOMPARE(settings.value(prefix + QStringLiteral("clip_count")).toString(), QStringLiteral("3"));
    }
    QVERIFY(!settings.settings().contains(QStringLiteral("should_not_run")));

    engine.unloadPlugins();
    const QList<PluginManifest> remainingPlugins = engine.loadedPlugins();
    const QList<PluginInfo> remainingInfos = engine.pluginInfos();
    for (const QString &pluginId : pluginIds) {
        QVERIFY(std::none_of(remainingPlugins.cbegin(), remainingPlugins.cend(), [&pluginId](const PluginManifest &plugin) {
            return plugin.id == pluginId;
        }));
        QVERIFY(std::none_of(remainingInfos.cbegin(), remainingInfos.cend(), [&pluginId](const PluginInfo &info) {
            return info.manifest.id == pluginId;
        }));
    }
    QVERIFY(engine.currentPluginId().isEmpty());
    QVERIFY(engine.state() != nullptr);
    for (const QString &pluginId : pluginIds) {
        QCOMPARE(settings.value(QStringLiteral("plugin.%1.unload").arg(pluginId)).toString(), QStringLiteral("called"));
    }
}

void TestModEngine::rejectsOversizedAndInvalidPlugins() {
    ModEngine &engine = ModEngine::instance();
    const QString pluginsPath = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("plugins"));
    const QString oversizedName = QStringLiteral("oversized_plugin.lua");
    const QString oversizedPath = QDir(pluginsPath).filePath(oversizedName);
    const QString invalidDirectory = QStringLiteral("invalid_identity_plugin");
    const QString incompatibleDirectory = QStringLiteral("incompatible_plugin");
    const QString duplicateFirstDirectory = QStringLiteral("duplicate_plugin_a");
    const QString duplicateSecondDirectory = QStringLiteral("duplicate_plugin_b");
    const QString invalidPath = QDir(pluginsPath).filePath(invalidDirectory);
    const QString incompatiblePath = QDir(pluginsPath).filePath(incompatibleDirectory);
    const QString duplicateFirstPath = QDir(pluginsPath).filePath(duplicateFirstDirectory);
    const QString duplicateSecondPath = QDir(pluginsPath).filePath(duplicateSecondDirectory);

    engine.unloadPlugins();
    QFile::remove(oversizedPath);
    QDir(invalidPath).removeRecursively();
    QDir(incompatiblePath).removeRecursively();
    QDir(duplicateFirstPath).removeRecursively();
    QDir(duplicateSecondPath).removeRecursively();
    const auto cleanup = qScopeGuard([&]() {
        engine.unloadPlugins();
        QFile::remove(oversizedPath);
        QDir(invalidPath).removeRecursively();
        QDir(incompatiblePath).removeRecursively();
        QDir(duplicateFirstPath).removeRecursively();
        QDir(duplicateSecondPath).removeRecursively();
    });

    QVERIFY(QDir().mkpath(pluginsPath));
    QFile oversized(oversizedPath);
    QVERIFY(oversized.open(QIODevice::WriteOnly));
    QVERIFY(oversized.write("function AviQtlOnLoad() end") > 0);
    QVERIFY(oversized.resize(AviQtl::Core::Internal::FileSizeLimit::PluginScript + 1));
    oversized.close();

    QVERIFY(QDir().mkpath(invalidPath));
    QVERIFY(writeTextFile(QDir(invalidPath).filePath(QStringLiteral("manifest.lua")), QStringLiteral(R"(
return { id = "../invalid", name = "Invalid", version = "1.0.0" }
)")));
    QVERIFY(writeTextFile(QDir(invalidPath).filePath(QStringLiteral("main.lua")), QStringLiteral("function AviQtlOnLoad() end")));

    QVERIFY(QDir().mkpath(incompatiblePath));
    QVERIFY(writeTextFile(QDir(incompatiblePath).filePath(QStringLiteral("manifest.lua")), QStringLiteral(R"(
return {
    id = "test.incompatible",
    name = "Incompatible",
    version = "1.0.0",
    min_app_version = "9999.0.0"
}
)")));
    QVERIFY(writeTextFile(QDir(incompatiblePath).filePath(QStringLiteral("main.lua")), QStringLiteral("function AviQtlOnLoad() end")));

    const QString duplicateManifest = QStringLiteral(R"(
return { id = "test.duplicate", name = "Duplicate", version = "1.0.0" }
)");
    for (const QString &path : {duplicateFirstPath, duplicateSecondPath}) {
        QVERIFY(QDir().mkpath(path));
        QVERIFY(writeTextFile(QDir(path).filePath(QStringLiteral("manifest.lua")), duplicateManifest));
        QVERIFY(writeTextFile(QDir(path).filePath(QStringLiteral("main.lua")), QStringLiteral("function AviQtlOnLoad() end")));
    }

    engine.loadPlugins();
    const QList<PluginInfo> infos = engine.pluginInfos();
    const auto containsPlugin = [&infos](const QString &pluginId) {
        return std::any_of(infos.cbegin(), infos.cend(), [&pluginId](const PluginInfo &info) {
            return info.manifest.id == pluginId;
        });
    };
    QVERIFY(!containsPlugin(QStringLiteral("file:") + oversizedName));
    QVERIFY(!containsPlugin(QStringLiteral("../invalid")));
    QVERIFY(!containsPlugin(QStringLiteral("test.incompatible")));
    QCOMPARE(std::count_if(infos.cbegin(), infos.cend(), [](const PluginInfo &info) {
                 return info.manifest.id == QStringLiteral("test.duplicate");
             }),
             1);

    const QVariantList installedPackages = AviQtl::Core::PackageManager::instance().getPackagesByType(QStringLiteral("installed"));
    QVERIFY(std::none_of(installedPackages.cbegin(), installedPackages.cend(), [&oversizedName](const QVariant &entry) {
        return entry.toMap().value(QStringLiteral("id")).toString() == QStringLiteral("file:") + oversizedName;
    }));
}

#include "test_mod_engine.moc"
QTEST_MAIN(TestModEngine)
