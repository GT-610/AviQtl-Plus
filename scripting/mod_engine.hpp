#pragma once
#include <QDir>
#include <QString>
#include <QVariantMap>
#include <lua.hpp>

namespace AviQtl::Scripting {

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
    QList<PluginManifest> m_loadedPlugins;
};

} // namespace AviQtl::Scripting