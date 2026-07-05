#include "settings_manager.hpp"
#include "constants.hpp"
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QStandardPaths>

Q_LOGGING_CATEGORY(lcSettings, "aviqtl.settings")

namespace AviQtl::Core {

namespace {
auto getDefaultPluginPaths(const QString &type, const QStringList &envVars, const QStringList &defaultDirs) -> QStringList {
    QStringList paths;
    for (const QString &envKey : envVars) {
        const QByteArray val = qgetenv(envKey.toUtf8().constData());
        if (!val.isEmpty()) {
            paths << QString::fromLocal8Bit(val).split(':', Qt::SkipEmptyParts);
        }
    }
    paths << (QDir::homePath() + QLatin1String("/.") + type);

    paths << type;

    paths << defaultDirs;
    paths.removeDuplicates();
    return paths;
}
} // namespace

auto SettingsManager::instance() -> SettingsManager & {
    static SettingsManager instance;
    return instance;
}

SettingsManager::SettingsManager(QObject *parent) : QObject(parent) {
    m_settings = {
        {"pluginEnableLADSPA", true},
        {"pluginPathsLADSPA", getDefaultPluginPaths(QStringLiteral("ladspa"), {QStringLiteral("LADSPA_PATH")}, {QStringLiteral("/usr/lib/ladspa"), QStringLiteral("/usr/local/lib/ladspa")})},
        {"pluginEnableDSSI", true},
        {"pluginPathsDSSI", getDefaultPluginPaths(QStringLiteral("dssi"), {QStringLiteral("DSSI_PATH")}, {QStringLiteral("/usr/lib/dssi"), QStringLiteral("/usr/local/lib/dssi")})},
        {"pluginEnableLV2", true},
        {"pluginPathsLV2", getDefaultPluginPaths(QStringLiteral("lv2"), {QStringLiteral("LV2_PATH")}, {QStringLiteral("/usr/lib/lv2"), QStringLiteral("/usr/local/lib/lv2")})},
        {"pluginEnableVST2", true},
        {"pluginPathsVST2", getDefaultPluginPaths(QStringLiteral("vst2"), {QStringLiteral("VST_PATH")}, {QStringLiteral("/usr/lib/vst"), QStringLiteral("/usr/lib/vst2"), QStringLiteral("/usr/local/lib/vst"), QStringLiteral("/usr/local/lib/vst2")})},
        {"pluginEnableVST3", true},
        {"pluginPathsVST3", getDefaultPluginPaths(QStringLiteral("vst3"), {QStringLiteral("VST3_PATH")}, {QStringLiteral("/usr/lib/vst3"), QStringLiteral("/usr/local/lib/vst3")})},
        {"pluginEnableCLAP", true},
        {"pluginPathsCLAP", getDefaultPluginPaths(QStringLiteral("clap"), {QStringLiteral("CLAP_PATH")}, {QStringLiteral("/usr/lib/clap"), QStringLiteral("/usr/local/lib/clap")})},
        {"pluginEnableSF2", true},
        {"pluginPathsSF2", getDefaultPluginPaths(QStringLiteral("sf2"), {QStringLiteral("SF2_PATH")}, {QStringLiteral("/usr/share/soundfonts"), QStringLiteral("/usr/share/sounds/sf2")})},
        {"pluginEnableSFZ", true},
        {"pluginPathsSFZ", getDefaultPluginPaths(QStringLiteral("sfz"), {QStringLiteral("SFZ_PATH")}, {QStringLiteral("/usr/share/sounds/sfz")})},
        {"pluginEnableJSFX", true},
        {"pluginPathsJSFX", getDefaultPluginPaths(QStringLiteral("jsfx"), {}, {})},
        {"pluginEnableEffects", true},
        {"pluginPathsEffects", getDefaultPluginPaths(QStringLiteral("effects"), {QStringLiteral("AVIQTL_EFFECTS_PATH")}, {})},
        {"pluginEnableObjects", true},
        {"pluginPathsObjects", getDefaultPluginPaths(QStringLiteral("objects"), {QStringLiteral("AVIQTL_OBJECTS_PATH")}, {})},
        {"pluginEnableTransitions", true},
        {"pluginPathsTransitions", getDefaultPluginPaths(QStringLiteral("transitions"), {QStringLiteral("AVIQTL_TRANSITIONS_PATH")}, {})},
        {"packageRepositories", QVariantList{QVariantMap{
            {QStringLiteral("url"), QStringLiteral("https://raw.githubusercontent.com/GT-610/AviQtl-Plus/main/repos/repo.json")},
            {QStringLiteral("name"), QStringLiteral("AviQtl Official")},
            {QStringLiteral("enabled"), true},
            {QStringLiteral("priority"), 10}
        }}},
        {"maxImageSize", 8192},
        {"cacheSize", 512},
        {"undoCount", 32},
        {"renderThreads", 0},
        {"theme", "Dark"},
        {"showConfirmOnClose", true},
        {"enableAutoBackup", true},
        {"backupInterval", 5},
        {"defaultProjectWidth", AviQtl::kDefaultWidth},
        {"defaultProjectHeight", AviQtl::kDefaultHeight},
        {"defaultProjectFps", AviQtl::kDefaultFps},
        {"defaultProjectFrames", 3600},
        {"defaultProjectSampleRate", AviQtl::kDefaultSampleRate},
        {"defaultClipDuration", AviQtl::kDefaultClipDuration},
        {"timeUnit", "frame"},
        {"enableSnap", true},
        {"enableTimelineSkimming", true},
        {"splitAtCursor", true},
        {"showLayerRange", true},
        {"timelineTrackHeight", 30},
        {"timelineHeaderHeight", 28},
        {"timelineRulerHeight", 32},
        {"timelineMaxLayers", 128},
        {"timelineLayerHeaderWidth", 60},
        {"timelineRulerTimeWidth", 70},
        {"timelineClipResizeHandleWidth", 10},
        {"splashDuration", 1000},
        {"splashSize", 512},
        {"appStartupDelay", 1000},
        {"exportImageQuality", 95},
        {"exportSequencePadding", 6},
        {"minClipDurationFrames", 5},
        {"magneticSnapRange", 10},
        {"timelineZoomMin", 10},
        {"timelineZoomMax", 400},
        {"timelineZoomStep", 10},
        {"videoDecoderIndexReserve", 108000},
        {"videoDecoderMinCacheMB", 64},
        {"hwFramePoolSize", 32},
        {"exportDefaultCodec", "h264_vaapi"},
        {"exportDefaultBitrateMbps", 15},
        {"exportDefaultCrf", 20},
        {"exportDefaultAudioCodec", "aac"},
        {"exportDefaultAudioBitrateKbps", 192},
        {"exportFrameGrabTimeoutMs", 2000},
        {"exportProgressInterval", 5},
        {"audioChannels", 2},
        {"audioPluginMaxBlockSize", AviQtl::kAudioMaxBlockSize},
        {"sceneWidthMax", 8000},
        {"sceneHeightMax", 8000},
        {"sceneFramesMin", 100},
        {"sceneFramesMax", 24000},
        {"sceneFramesStep", 100},
        {"recentProjectMaxCount", 10},
        {"luaHookIntervalMs", 16},
        {"textPaddingMultiplier", 4.0},
        {"shortcuts", defaultShortcutSettings()}};
    load();
}

auto SettingsManager::defaultShortcutSettings() -> QVariantMap {
    return {// Project
            {"project.new", "Ctrl+N"},
            {"project.save", "Ctrl+S"},
            {"project.open", "Ctrl+O"},
            {"project.saveAs", "Ctrl+Shift+S"},
            {"project.export", "Ctrl+E"},
            {"app.quit", "Ctrl+Q"},
            {"app.settings", "Ctrl+P"},

            // Edit
            {"edit.undo", "Ctrl+Z"},
            {"edit.redo", "Ctrl+Shift+Z"},
            {"edit.cut", "Ctrl+X"},
            {"edit.copy", "Ctrl+C"},
            {"edit.paste", "Ctrl+V"},
            {"edit.delete", "Delete"},
            {"edit.duplicate", "Ctrl+D"},

            // Transport
            {"transport.playPause", "Space"},
            {"transport.nextFrame", "Right"},
            {"transport.prevFrame", "Left"},
            {"transport.jumpStart", "Home"},
            {"transport.jumpEnd", "End"},

            // View
            {"view.zoomIn", "Ctrl++"},
            {"view.zoomOut", "Ctrl+-"},
            {"view.timeline", "F3"},
            {"view.objectSettings", "F4"},
            {"project.settings", "Alt+Enter"},

            // Timeline
            {"timeline.split", "S"},
            {"timeline.moveUp", "Alt+Up"},
            {"timeline.moveDown", "Alt+Down"},
            {"timeline.nudgeLeft", "Alt+Left"},
            {"timeline.nudgeRight", "Alt+Right"},
            {"timeline.addScene", "Ctrl+T"},
            {"timeline.sceneSettings", "Alt+S"},
            {"timeline.removeScene", "Ctrl+Shift+Delete"},
            {"timeline.layerLock", "Ctrl+L"},
            {"timeline.layerHide", "Ctrl+H"}};
}

auto SettingsManager::getSettingsFilePath() -> QString {
    QString exeDir = QCoreApplication::applicationDirPath();
    QString portablePath = exeDir + QLatin1String("/aviqtl_settings.json");

    // 書き込み可能かチェック
    QFile file(portablePath);
    if (file.exists()) {
        if (!file.permissions().testFlag(QFile::WriteUser)) {
            qWarning() << "Portable settings file found but not writable. Falling back.";
        } else {
            return portablePath;
        }
    } else {
        // 存在しない場合は、ディレクトリの権限をチェック
        QFileInfo dirInfo(exeDir);
        if (dirInfo.isWritable()) {
            return portablePath;
        }
    }

    QString dataPath = QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation);
    QDir().mkpath(dataPath);
    return dataPath + QLatin1String("/settings.json");
}

void SettingsManager::setSettings(const QVariantMap &settings) {
    if (m_settings != settings) {
        m_settings = settings;
        emit settingsChanged();
        save(); // 変更時に自動保存
    }
}

void SettingsManager::load() {
    QString path = getSettingsFilePath();
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        qCInfo(lcSettings) << "Settings file not found:" << path << ". Using default values.";
        return;
    }

    QJsonDocument doc = QJsonDocument::fromJson(file.readAll());
    if (doc.isObject()) {
        QVariantMap loaded = doc.object().toVariantMap();
        for (auto it = loaded.begin(); it != loaded.end(); ++it) {
            if (it.key() == QLatin1String("shortcuts") && it.value().canConvert<QVariantMap>()) {
                QVariantMap mergedShortcuts = m_settings.value(QStringLiteral("shortcuts")).toMap();
                QVariantMap loadedShortcuts = it.value().toMap();
                for (auto shortcutIt = loadedShortcuts.begin(); shortcutIt != loadedShortcuts.end(); ++shortcutIt) {
                    mergedShortcuts.insert(shortcutIt.key(), shortcutIt.value());
                }
                m_settings.insert(it.key(), mergedShortcuts);
                continue;
            }
            m_settings.insert(it.key(), it.value());
        }
        // Migration: legacy packageRepositoryUrls → packageRepositories
        if (loaded.contains(QStringLiteral("packageRepositoryUrls"))) {
            if (!loaded.contains(QStringLiteral("packageRepositories"))) {
                QStringList oldUrls = loaded.value(QStringLiteral("packageRepositoryUrls")).toStringList();
                QVariantList newRepos;
                for (const QString &oldUrl : oldUrls) {
                    QVariantMap repo;
                    repo[QStringLiteral("url")] = oldUrl;
                    repo[QStringLiteral("name")] = oldUrl;
                    repo[QStringLiteral("enabled")] = true;
                    repo[QStringLiteral("priority")] = 10;
                    newRepos.append(repo);
                }
                m_settings.insert(QStringLiteral("packageRepositories"), newRepos);
                qCInfo(lcSettings) << "Migrated packageRepositoryUrls to packageRepositories";
            }
            m_settings.remove(QStringLiteral("packageRepositoryUrls"));
            save();
        }

        emit settingsChanged();
        qCInfo(lcSettings) << "Settings loaded:" << path;
    }
}

void SettingsManager::save() {
    QString path = getSettingsFilePath();

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        qWarning() << "Failed to save settings (cannot open file):" << path;
        return;
    }

    QJsonObject obj = QJsonObject::fromVariantMap(m_settings);
    QJsonDocument doc(obj);
    const QByteArray payload = doc.toJson();
    qint64 written = file.write(payload);

    if (written != payload.size()) {
        qWarning() << "Failed to write settings:" << path;
        file.cancelWriting();
        return;
    }

    if (!file.commit()) {
        qWarning() << "Failed to commit settings:" << path;
        return;
    }

    qCInfo(lcSettings) << "Settings saved:" << path;
}

void SettingsManager::setValue(const QString &key, const QVariant &value) {
    if (m_settings.value(key) != value) {
        m_settings.insert(key, value);
        emit settingsChanged();
        // Runtime keys starting with "_" are not saved to disk
        if (!key.startsWith(QStringLiteral("_"))) {
            save();
        }
    }
}

auto SettingsManager::value(const QString &key, const QVariant &defaultValue) const -> QVariant { return m_settings.value(key, defaultValue); }

auto SettingsManager::shortcuts() const -> QVariantMap { return m_settings.value(QStringLiteral("shortcuts")).toMap(); }

auto SettingsManager::shortcut(const QString &actionId, const QString &fallbackValue) const -> QString {
    const QVariantMap shortcutMap = shortcuts();
    const QString value = shortcutMap.value(actionId, fallbackValue).toString();
    return value.isEmpty() ? fallbackValue : value;
}

} // namespace AviQtl::Core
