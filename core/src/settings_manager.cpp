#include "settings_manager.hpp"
#include "rust_settings_document.hpp"
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
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

auto platformDefaultSettings() -> QVariantMap {
    return {
        {"pluginPathsLADSPA", getDefaultPluginPaths(QStringLiteral("ladspa"), {QStringLiteral("LADSPA_PATH")}, {QStringLiteral("/usr/lib/ladspa"), QStringLiteral("/usr/local/lib/ladspa")})},
        {"pluginPathsDSSI", getDefaultPluginPaths(QStringLiteral("dssi"), {QStringLiteral("DSSI_PATH")}, {QStringLiteral("/usr/lib/dssi"), QStringLiteral("/usr/local/lib/dssi")})},
        {"pluginPathsLV2", getDefaultPluginPaths(QStringLiteral("lv2"), {QStringLiteral("LV2_PATH")}, {QStringLiteral("/usr/lib/lv2"), QStringLiteral("/usr/local/lib/lv2")})},
        {"pluginPathsVST2", getDefaultPluginPaths(QStringLiteral("vst2"), {QStringLiteral("VST_PATH")}, {QStringLiteral("/usr/lib/vst"), QStringLiteral("/usr/lib/vst2"), QStringLiteral("/usr/local/lib/vst"), QStringLiteral("/usr/local/lib/vst2")})},
        {"pluginPathsVST3", getDefaultPluginPaths(QStringLiteral("vst3"), {QStringLiteral("VST3_PATH")}, {QStringLiteral("/usr/lib/vst3"), QStringLiteral("/usr/local/lib/vst3")})},
        {"pluginPathsCLAP", getDefaultPluginPaths(QStringLiteral("clap"), {QStringLiteral("CLAP_PATH")}, {QStringLiteral("/usr/lib/clap"), QStringLiteral("/usr/local/lib/clap")})},
        {"pluginPathsSF2", getDefaultPluginPaths(QStringLiteral("sf2"), {QStringLiteral("SF2_PATH")}, {QStringLiteral("/usr/share/soundfonts"), QStringLiteral("/usr/share/sounds/sf2")})},
        {"pluginPathsSFZ", getDefaultPluginPaths(QStringLiteral("sfz"), {QStringLiteral("SFZ_PATH")}, {QStringLiteral("/usr/share/sounds/sfz")})},
        {"pluginPathsJSFX", getDefaultPluginPaths(QStringLiteral("jsfx"), {}, {})},
        {"pluginPathsEffects", getDefaultPluginPaths(QStringLiteral("effects"), {QStringLiteral("AVIQTL_EFFECTS_PATH")}, {})},
        {"pluginPathsObjects", getDefaultPluginPaths(QStringLiteral("objects"), {QStringLiteral("AVIQTL_OBJECTS_PATH")}, {})},
    };
}
} // namespace

auto SettingsManager::instance() -> SettingsManager & {
    static SettingsManager instance;
    return instance;
}

SettingsManager::SettingsManager(QObject *parent) : QObject(parent) {
    const auto defaults = RustCore::Settings::defaults(platformDefaultSettings());
    if (!defaults.has_value())
        qFatal("[SettingsManager] Rust core failed to construct the settings document");
    m_settings = *defaults;
    load();
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

    const auto merged = RustCore::Settings::merge(m_settings, file.readAll());
    if (!merged.has_value()) {
        qWarning() << "Failed to parse settings:" << path;
        return;
    }
    m_settings = merged->settings;
    if (merged->migrated) {
        qCInfo(lcSettings) << "Migrated packageRepositoryUrls to packageRepositories";
        save();
    }
    emit settingsChanged();
    qCInfo(lcSettings) << "Settings loaded:" << path;
}

void SettingsManager::save() {
    QString path = getSettingsFilePath();

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        qWarning() << "Failed to save settings (cannot open file):" << path;
        return;
    }

    const auto persistentJson = RustCore::Settings::persistentJson(m_settings);
    if (!persistentJson.has_value()) {
        qWarning() << "Failed to serialize settings:" << path;
        file.cancelWriting();
        return;
    }
    const QByteArray &payload = *persistentJson;
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

void SettingsManager::removeValue(const QString &key) {
    if (m_settings.remove(key) > 0) {
        emit settingsChanged();
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
