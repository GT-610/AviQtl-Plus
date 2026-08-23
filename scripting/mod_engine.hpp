#pragma once
#include <QByteArray>
#include <QDir>
#include <QFileSystemWatcher>
#include <QHash>
#include <QMetaObject>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QTimer>
#include <QVariantMap>
#include <lua.hpp>

#include "script_params.hpp"

namespace AviQtl::Core {
class PermissionManager;
}

namespace AviQtl::UI {
class TimelineController;
}

namespace AviQtl::Scripting {

class PluginFileWatcher : public QObject {
    Q_OBJECT
  public:
    explicit PluginFileWatcher(QObject *parent = nullptr) : QObject(parent) {
        connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, &PluginFileWatcher::directoryChanged);
    }

    void watchPath(const QString &path) {
        if (!m_watcher.directories().contains(path)) {
            m_watcher.addPath(path);
        }
    }

    void clearPaths() {
        for (const QString &path : m_watcher.directories()) {
            m_watcher.removePath(path);
        }
    }

  signals:
    void directoryChanged(const QString &path);

  private:
    QFileSystemWatcher m_watcher;
};

struct PluginManifest {
    QString id;
    QString name;
    QString version;
    QString author;
    QString description;
    QString minAppVersion;
    bool isValid() const { return !id.isEmpty() && !name.isEmpty() && !version.isEmpty(); }
};

struct PluginInfo {
    PluginManifest manifest;
    ScriptMetadata scriptMeta;
    QString filePath;
    QVariantMap paramValues; // Current parameter values
};

class ModEngine {
  public:
    static ModEngine &instance();

    void initialize();
    // TimelineController を登録 (main.cpp の QML登録後に呼ぶ)
    void registerController(AviQtl::UI::TimelineController *controller);
    void loadPlugins();
    void onUpdate();

    // Plugin management
    PluginManifest loadManifest(const QString &pluginDir);
    ScriptMetadata loadScriptParams(const QString &scriptPath);
    QList<PluginManifest> loadedPlugins() const { return m_loadedPlugins; }
    QList<PluginInfo> pluginInfos() const { return m_pluginInfos; }
    void unloadPlugins();

    // Script parameters
    Q_INVOKABLE QVariantMap getPluginParams(const QString &pluginId) const;
    Q_INVOKABLE void setPluginParam(const QString &pluginId, const QString &key, const QVariant &value);
    void injectPluginParams(lua_State *L, const PluginInfo &info);

    // Hot reload
    void enableHotReload(bool enable);

    // Permission checking
    bool checkPermission(const char *apiName) const;
    QString currentPluginId() const { return m_currentPluginId; }

    // Lifecycle hooks
    void onLoad();
    void onUnload();
    void onProjectOpen(const QString &path);
    void onProjectSave(const QString &path);
    void onClipChange();

    lua_State *state() { return L; }

  private:
    ModEngine() = default;
    ~ModEngine();
    ModEngine(const ModEngine &) = delete;
    ModEngine &operator=(const ModEngine &) = delete;
    lua_State *L = nullptr;
    bool m_apiRegistered = false;
    void registerAviQtlAPI();
    void callHooks(const char *hookName, const QString *argument = nullptr);
    void capturePluginHooks(const QString &pluginId);
    void releasePluginHooks();
    void clearHookGlobals();
    void resetLuaState();
    void setupFileWatcher();
    void onPluginDirectoryChanged(const QString &path);
    void loadSingleFilePlugin(const QFileInfo &fileInfo);
    void loadDirectoryPlugin(const QString &subdir, const QString &pluginsPath);
    bool loadPlugin(const PluginManifest &manifest, const QString &scriptPath, bool singleFile);
    bool validatePlugin(const PluginManifest &manifest, const QString &scriptPath, bool singleFile) const;
    QList<PluginManifest> m_loadedPlugins;
    QList<PluginInfo> m_pluginInfos;
    struct PluginRuntime {
        QString pluginId;
        QHash<QByteArray, int> hookRefs;
    };
    QList<PluginRuntime> m_pluginRuntimes;
    PluginFileWatcher *m_fileWatcher = nullptr;
    QMetaObject::Connection m_clipChangeConnection;
    QTimer m_reloadDebounceTimer;
    bool m_hotReloadEnabled = false;
    bool m_dispatchingHooks = false;
    QString m_currentPluginId;
    bool m_initialized = false;
};

} // namespace AviQtl::Scripting
