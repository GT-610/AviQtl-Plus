#include "mod_engine.hpp"
#include "permission_manager.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QMap>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QTextStream>

using namespace AviQtl::Scripting;

class TestModEngine : public QObject {
    Q_OBJECT

  private slots:
    void pluginFileWatcherWatchesAndClearsPaths();
    void apiPermissionMappingCoversRegisteredOperations();
    void loadsAndDispatchesEachPluginLifecycle();
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
                  const QString &pluginId, const QString &pluginName) {
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
    record("clip", "called")
end

function AviQtlOnUnload()
    record("unload", "called")
end
)");
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
    QVERIFY(createPlugin(pluginsPath, pluginDirectories.at(0), pluginIds.at(0), QStringLiteral("Lifecycle Alpha")));
    QVERIFY(createPlugin(pluginsPath, pluginDirectories.at(1), pluginIds.at(1), QStringLiteral("Lifecycle Beta")));
    const QString invalidPath = QDir(pluginsPath).filePath(pluginDirectories.at(2));
    QVERIFY(QDir().mkpath(invalidPath));
    QVERIFY(writeTextFile(QDir(invalidPath).filePath(QStringLiteral("main.lua")),
                          QStringLiteral("aviqtl.settings.set('should_not_run', 'yes')")));

    for (const QString &pluginId : pluginIds) {
        permissions.revokeAllPermissions(pluginId);
        permissions.grantPermission(pluginId, PluginPermission::SettingsWrite);
    }

    engine.loadPlugins();
    QCOMPARE(engine.loadedPlugins().size(), 2);
    QCOMPARE(engine.pluginInfos().size(), 2);
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

    for (const QString &pluginId : pluginIds) {
        const QString prefix = QStringLiteral("plugin.%1.").arg(pluginId);
        QCOMPARE(settings.value(prefix + QStringLiteral("load")).toString(), QStringLiteral("called"));
        QCOMPARE(settings.value(prefix + QStringLiteral("update")).toString(), QStringLiteral("called"));
        QCOMPARE(settings.value(prefix + QStringLiteral("open")).toString(), openedPath);
        QCOMPARE(settings.value(prefix + QStringLiteral("save")).toString(), savedPath);
        QCOMPARE(settings.value(prefix + QStringLiteral("clip")).toString(), QStringLiteral("called"));
    }
    QVERIFY(!settings.settings().contains(QStringLiteral("should_not_run")));

    engine.unloadPlugins();
    QCOMPARE(engine.loadedPlugins().size(), 0);
    QCOMPARE(engine.pluginInfos().size(), 0);
    QVERIFY(engine.currentPluginId().isEmpty());
    QVERIFY(engine.state() != nullptr);
    for (const QString &pluginId : pluginIds) {
        QCOMPARE(settings.value(QStringLiteral("plugin.%1.unload").arg(pluginId)).toString(), QStringLiteral("called"));
    }
}

#include "test_mod_engine.moc"
QTEST_MAIN(TestModEngine)
