#include "settings_manager.hpp"
#include "rust_settings_document.hpp"
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QSaveFile>
#include <QStandardPaths>
#include <utility>

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
    // Defaults are required to establish the complete settings schema; continuing with a
    // partial map would make later reads silently depend on unrelated caller fallbacks.
    if (m_state.initializeDefaults(platformDefaultSettings()) !=
            RustCore::Settings::Status::Ok ||
        !syncProjection())
        qFatal("[SettingsManager] Rust core failed to construct the settings document");
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
        if (m_state.reset(settings) != RustCore::Settings::Status::Ok || !syncProjection()) {
            qWarning() << "Failed to replace Rust settings state";
            return;
        }
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

    const QByteArray contents = file.readAll();
    file.close();
    bool migrated = false;
    if (m_state.mergeLoaded(contents, migrated) != RustCore::Settings::Status::Ok ||
        !syncProjection()) {
        qWarning() << "Failed to parse settings:" << path;
        return;
    }
    if (migrated) {
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

    QByteArray payload;
    if (m_state.persistentJson(payload) != RustCore::Settings::Status::Ok) {
        qWarning() << "Failed to serialize settings:" << path;
        file.cancelWriting();
        return;
    }
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
    bool changed = false;
    bool persistent = false;
    if (m_state.setValue(key, value, changed, persistent) != RustCore::Settings::Status::Ok) {
        qWarning() << "Failed to update Rust settings state for key:" << key;
        return;
    }
    if (changed) {
        if (!syncProjection()) {
            qCritical() << "Failed to project Rust settings state after updating key:" << key;
            return;
        }
        emit settingsChanged();
        // Runtime keys starting with "_" are not saved to disk
        if (persistent) {
            save();
        }
    }
}

void SettingsManager::removeValue(const QString &key) {
    bool changed = false;
    bool persistent = false;
    if (m_state.removeValue(key, changed, persistent) != RustCore::Settings::Status::Ok) {
        qWarning() << "Failed to remove key from Rust settings state:" << key;
        return;
    }
    if (changed) {
        if (!syncProjection()) {
            qCritical() << "Failed to project Rust settings state after removing key:" << key;
            return;
        }
        emit settingsChanged();
        if (persistent) {
            save();
        }
    }
}

auto SettingsManager::value(const QString &key, const QVariant &defaultValue) const -> QVariant { return m_settings.value(key, defaultValue); }

auto SettingsManager::intValue(const QString &key, int defaultValue) const -> int {
    return m_state.intValue(key, defaultValue);
}

auto SettingsManager::doubleValue(const QString &key, double defaultValue) const -> double {
    return m_state.doubleValue(key, defaultValue);
}

auto SettingsManager::boolValue(const QString &key, bool defaultValue) const -> bool {
    return m_state.boolValue(key, defaultValue);
}

auto SettingsManager::shortcuts() const -> QVariantMap { return m_settings.value(QStringLiteral("shortcuts")).toMap(); }

auto SettingsManager::shortcut(const QString &actionId, const QString &fallbackValue) const -> QString {
    const QVariantMap shortcutMap = shortcuts();
    const QString value = shortcutMap.value(actionId, fallbackValue).toString();
    return value.isEmpty() ? fallbackValue : value;
}

bool SettingsManager::syncProjection() {
    QVariantMap projected;
    if (m_state.snapshot(projected) != RustCore::Settings::Status::Ok)
        return false;
    m_settings = std::move(projected);
    return true;
}

} // namespace AviQtl::Core
