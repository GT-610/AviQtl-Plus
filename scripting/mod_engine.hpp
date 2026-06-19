#pragma once
#include <QDir>
#include <QFileSystemWatcher>
#include <QObject>
#include <QString>
#include <QVariantMap>
#include <lua.hpp>

namespace AviQtl::Core {
class PermissionManager;
}

namespace AviQtl::Scripting {

class PluginFileWatcher : public QObject {
    Q_OBJECT
  public:
    explicit PluginFileWatcher(QObject *parent = nullptr) : QObject(parent) {}

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

struct HostApiTable {
    void (*log)(const char *msg);

    // Transport
    void (*transport_play)();
    void (*transport_pause)();
    void (*transport_toggle)();
    void (*transport_seek)(int frame);
    int (*transport_get_frame)();
    int (*transport_is_playing)();

    // Clip
    void (*clip_create)(const char *type, int startFrame, int layer);
    void (*clip_delete)(int clipId);
    void (*clip_update)(int clipId, int layer, int startFrame, int duration);
    void (*clip_select)(int clipId);

    // Project
    int (*project_get_width)();
    int (*project_get_height)();
    double (*project_get_fps)();

    // Scene
    void (*scene_create)(const char *name);
    void (*scene_switch)(int sceneId);

    // Settings
    void (*settings_set)(const char *key, const char *value);
    const char *(*settings_get)(const char *key);

    // Command (Undo/Redo Grouping)
    void (*command_begin_group)(const char *text);
    void (*command_end_group)();
};

class ModEngine {
  public:
    static ModEngine &instance();

    void initialize(void *ecsPtr);
    // TimelineController を登録 (main.cpp の QML登録後に呼ぶ)
    void registerController(void *controller);
    void loadPlugins();
    void onUpdate();

    // Plugin management
    PluginManifest loadManifest(const QString &pluginDir);
    QList<PluginManifest> loadedPlugins() const { return m_loadedPlugins; }
    void unloadPlugins();

    // Hot reload
    void enableHotReload(bool enable);
    bool isHotReloadEnabled() const { return m_hotReloadEnabled; }

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
    void _registerAviQtlAPI();
    void _callHook(const char *hookName, int nargs = 0);
    void _setupFileWatcher();
    void _onPluginDirectoryChanged(const QString &path);
    QList<PluginManifest> m_loadedPlugins;
    PluginFileWatcher *m_fileWatcher = nullptr;
    bool m_hotReloadEnabled = false;
    QString m_currentPluginId;
};

} // namespace AviQtl::Scripting