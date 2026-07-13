#pragma once
#include <QDir>
#include <QFileSystemWatcher>
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

    void initialize(void *ecsPtr);
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
    void setCurrentPluginId(const QString &pluginId) { m_currentPluginId = pluginId; }
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
    void callHook(const char *hookName, int nargs = 0);
    void setupFileWatcher();
    void onPluginDirectoryChanged(const QString &path);
    void loadSingleFilePlugin(const QFileInfo &fileInfo);
    void loadDirectoryPlugin(const QString &subdir, const QString &pluginsPath);
    QList<PluginManifest> m_loadedPlugins;
    QList<PluginInfo> m_pluginInfos;
    PluginFileWatcher *m_fileWatcher = nullptr;
    QTimer m_reloadDebounceTimer;
    bool m_hotReloadEnabled = false;
    QString m_currentPluginId;
    QString m_lastLoadedPluginId; // context for hook dispatch
};

} // namespace AviQtl::Scripting