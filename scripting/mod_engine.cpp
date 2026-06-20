#include "mod_engine.hpp"
#include "lua_host.hpp"
#include "../ui/include/timeline_controller.hpp"
#include "../core/include/settings_manager.hpp"
#include "../core/include/permission_manager.hpp"
#include <QCoreApplication>
#include <QDebug>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMetaObject>
#include <QVariant>

namespace AviQtl::Scripting {

// Lua から参照できるグローバルポインタ
static AviQtl::UI::TimelineController *g_ctrl = nullptr;

// C API Wrappers for HostApiTable
extern "C" {
static void api_log(const char *msg) {
    if (!ModEngine::instance().checkPermission("log")) {
        return; // Silently deny log output
    }
    if (g_ctrl != nullptr) {
        AviQtl::UI::TimelineController::log(QString::fromUtf8(msg));
    }
}
static void api_transport_play() {
    if ((g_ctrl != nullptr) && !g_ctrl->transport()->isPlaying()) {
        g_ctrl->transport()->togglePlay();
    }
}
static void api_transport_pause() {
    if ((g_ctrl != nullptr) && g_ctrl->transport()->isPlaying()) {
        g_ctrl->transport()->togglePlay();
    }
}
static void api_transport_toggle() {
    if (g_ctrl != nullptr) {
        g_ctrl->transport()->togglePlay();
    }
}
static void api_transport_seek(int frame) {
    if (g_ctrl != nullptr) {
        g_ctrl->transport()->setCurrentFrame(frame);
    }
}
static auto api_transport_get_frame() -> int { return (g_ctrl != nullptr) ? g_ctrl->transport()->currentFrame() : 0; }
static auto api_transport_is_playing() -> int { return (g_ctrl != nullptr) ? (int)g_ctrl->transport()->isPlaying() : 0; }

static void api_clip_create(const char *type, int start, int layer) {
    if (g_ctrl != nullptr) {
        g_ctrl->createObject(QString::fromUtf8(type), start, layer);
    }
}
static void api_clip_delete(int id) {
    if (g_ctrl != nullptr) {
        g_ctrl->deleteClip(id);
    }
}
static void api_clip_update(int id, int layer, int start, int dur) {
    if (g_ctrl != nullptr) {
        g_ctrl->updateClip(id, layer, start, dur);
    }
}
static void api_clip_select(int id) {
    if (g_ctrl != nullptr) {
        g_ctrl->selectClip(id);
    }
}

static auto api_project_get_width() -> int { return (g_ctrl != nullptr) ? g_ctrl->project()->width() : 0; }
static auto api_project_get_height() -> int { return (g_ctrl != nullptr) ? g_ctrl->project()->height() : 0; }
static auto api_project_get_fps() -> double { return (g_ctrl != nullptr) ? g_ctrl->project()->fps() : 0.0; }

static void api_scene_create(const char *name) {
    if (g_ctrl != nullptr) {
        g_ctrl->createScene(QString::fromUtf8(name));
    }
}
static void api_scene_switch(int id) {
    if (g_ctrl != nullptr) {
        g_ctrl->switchScene(id);
    }
}

static void api_command_begin_group(const char *text) {
    if ((g_ctrl != nullptr) && (g_ctrl->timeline() != nullptr)) {
        g_ctrl->timeline()->undoStack()->beginMacro(QString::fromUtf8(text));
    }
}
static void api_command_end_group() {
    if ((g_ctrl != nullptr) && (g_ctrl->timeline() != nullptr)) {
        g_ctrl->timeline()->undoStack()->endMacro();
    }
}

// Settings API (scoped to current plugin)
static char g_settings_buf[4096]; // Buffer for returning strings
static void api_settings_set(const char *key, const char *value) {
    const QString &pluginId = ModEngine::instance().currentPluginId();
    QString scopedKey = pluginId.isEmpty() ? QString::fromUtf8(key) : QStringLiteral("plugin.%1.%2").arg(pluginId, QString::fromUtf8(key));
    AviQtl::Core::SettingsManager::instance().setValue(scopedKey, QString::fromUtf8(value));
}
static const char *api_settings_get(const char *key) {
    const QString &pluginId = ModEngine::instance().currentPluginId();
    QString scopedKey = pluginId.isEmpty() ? QString::fromUtf8(key) : QStringLiteral("plugin.%1.%2").arg(pluginId, QString::fromUtf8(key));
    QString val = AviQtl::Core::SettingsManager::instance().value(scopedKey).toString();
    QByteArray bytes = val.toUtf8();
    if (bytes.size() >= (int)sizeof(g_settings_buf)) {
        bytes.resize(sizeof(g_settings_buf) - 1);
    }
    memcpy(g_settings_buf, bytes.constData(), bytes.size());
    g_settings_buf[bytes.size()] = '\0';
    return g_settings_buf;
}
}

static HostApiTable g_hostApi = {.log = api_log,
                                 .transport_play = api_transport_play,
                                 .transport_pause = api_transport_pause,
                                 .transport_toggle = api_transport_toggle,
                                 .transport_seek = api_transport_seek,
                                 .transport_get_frame = api_transport_get_frame,
                                 .transport_is_playing = api_transport_is_playing,
                                 .clip_create = api_clip_create,
                                 .clip_delete = api_clip_delete,
                                 .clip_update = api_clip_update,
                                 .clip_select = api_clip_select,
                                 .project_get_width = api_project_get_width,
                                 .project_get_height = api_project_get_height,
                                 .project_get_fps = api_project_get_fps,
                                 .scene_create = api_scene_create,
                                 .scene_switch = api_scene_switch,
                                 .settings_set = api_settings_set,
                                 .settings_get = api_settings_get,
                                 .command_begin_group = api_command_begin_group,
                                 .command_end_group = api_command_end_group};

bool ModEngine::checkPermission(const char *apiName) const {
    if (m_currentPluginId.isEmpty()) {
        return true; // No plugin context, allow (for direct calls)
    }

    auto &permMgr = AviQtl::Core::PermissionManager::instance();

    // Map API names to permissions
    struct ApiPermMap {
        const char *prefix;
        AviQtl::Core::PluginPermission permission;
    };

    static const ApiPermMap apiPermMap[] = {
        {"transport", AviQtl::Core::PluginPermission::TransportControl},
        {"clip_list", AviQtl::Core::PluginPermission::ClipRead},
        {"clip_create", AviQtl::Core::PluginPermission::ClipModify},
        {"clip_delete", AviQtl::Core::PluginPermission::ClipModify},
        {"clip_update", AviQtl::Core::PluginPermission::ClipModify},
        {"clip_select", AviQtl::Core::PluginPermission::ClipRead},
        {"clip_split", AviQtl::Core::PluginPermission::ClipModify},
        {"clip_copy", AviQtl::Core::PluginPermission::ClipboardAccess},
        {"clip_cut", AviQtl::Core::PluginPermission::ClipboardAccess},
        {"clip_paste", AviQtl::Core::PluginPermission::ClipboardAccess},
        {"effect_add", AviQtl::Core::PluginPermission::EffectModify},
        {"effect_remove", AviQtl::Core::PluginPermission::EffectModify},
        {"effect_set_param", AviQtl::Core::PluginPermission::EffectModify},
        {"project_width", AviQtl::Core::PluginPermission::ProjectRead},
        {"project_height", AviQtl::Core::PluginPermission::ProjectRead},
        {"project_fps", AviQtl::Core::PluginPermission::ProjectRead},
        {"project_save", AviQtl::Core::PluginPermission::ProjectSave},
        {"project_load", AviQtl::Core::PluginPermission::ProjectLoad},
        {"scene_create", AviQtl::Core::PluginPermission::SceneManage},
        {"scene_remove", AviQtl::Core::PluginPermission::SceneManage},
        {"scene_switch", AviQtl::Core::PluginPermission::SceneManage},
        {"settings_set", AviQtl::Core::PluginPermission::SettingsWrite},
        {"settings_get", AviQtl::Core::PluginPermission::SettingsRead},
        {"log", AviQtl::Core::PluginPermission::LogOutput},
    };

    for (const auto &entry : apiPermMap) {
        if (strncmp(apiName, entry.prefix, strlen(entry.prefix)) == 0) {
            return permMgr.hasPermission(m_currentPluginId, entry.permission);
        }
    }

    // Default: allow if no specific permission required
    return true;
}

// ヘルパー
static auto _checkCtrl(lua_State *L) -> int {
    if (g_ctrl == nullptr) {
        lua_pushstring(L, "[AviQtlAPI] controller not ready");
        lua_error(L);
    }
    return 0;
}

// transport
static auto l_transport_play(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("transport_play")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: transport.control");
        return lua_error(L);
    }
    if (!g_ctrl->transport()->isPlaying()) {
        g_ctrl->transport()->togglePlay();
    }
    return 0;
}

static auto l_log(lua_State *L) -> int {
    const char *msg = luaL_checkstring(L, 1);
    api_log(msg);
    return 0;
}

static auto l_transport_pause(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("transport_pause")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: transport.control");
        return lua_error(L);
    }
    if (g_ctrl->transport()->isPlaying()) {
        g_ctrl->transport()->togglePlay();
    }
    return 0;
}
static auto l_transport_toggle(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("transport_toggle")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: transport.control");
        return lua_error(L);
    }
    g_ctrl->transport()->togglePlay();
    return 0;
}
static auto l_transport_seek(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("transport_seek")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: transport.control");
        return lua_error(L);
    }
    int frame = static_cast<int>(luaL_checkinteger(L, 1));
    g_ctrl->transport()->setCurrentFrame(frame);
    return 0;
}
static auto l_transport_get_frame(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("transport_get_frame")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: transport.control");
        return lua_error(L);
    }
    lua_pushinteger(L, g_ctrl->transport()->currentFrame());
    return 1;
}
static auto l_transport_is_playing(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("transport_is_playing")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: transport.control");
        return lua_error(L);
    }
    lua_pushboolean(L, static_cast<int>(g_ctrl->transport()->isPlaying()));
    return 1;
}

// clip
static auto l_clip_create(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_create")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clip.modify");
        return lua_error(L);
    }
    // aviqtl_clip_create(type, startFrame, layer)
    const char *type = luaL_checkstring(L, 1);
    int startFrame = static_cast<int>(luaL_checkinteger(L, 2));
    int layer = static_cast<int>(luaL_checkinteger(L, 3));
    g_ctrl->createObject(QString::fromUtf8(type), startFrame, layer);
    return 0;
}
static auto l_clip_delete(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_delete")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clip.modify");
        return lua_error(L);
    }
    int clipId = static_cast<int>(luaL_checkinteger(L, 1));
    g_ctrl->deleteClip(clipId);
    return 0;
}
static auto l_clip_update(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_update")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clip.modify");
        return lua_error(L);
    }
    // aviqtl_clip_update(clipId, layer, startFrame, duration)
    int id = static_cast<int>(luaL_checkinteger(L, 1));
    int layer = static_cast<int>(luaL_checkinteger(L, 2));
    int start = static_cast<int>(luaL_checkinteger(L, 3));
    int dur = static_cast<int>(luaL_checkinteger(L, 4));
    g_ctrl->updateClip(id, layer, start, dur);
    return 0;
}
static auto l_clip_select(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_select")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clip.read");
        return lua_error(L);
    }
    g_ctrl->selectClip(static_cast<int>(luaL_checkinteger(L, 1)));
    return 0;
}
static auto l_clip_split(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_split")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clip.modify");
        return lua_error(L);
    }
    g_ctrl->splitClip(static_cast<int>(luaL_checkinteger(L, 1)), static_cast<int>(luaL_checkinteger(L, 2)));
    return 0;
}
static auto l_clip_copy(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_copy")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clipboard.access");
        return lua_error(L);
    }
    g_ctrl->copyClip(static_cast<int>(luaL_checkinteger(L, 1)));
    return 0;
}
static auto l_clip_cut(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_cut")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clipboard.access");
        return lua_error(L);
    }
    g_ctrl->cutClip(static_cast<int>(luaL_checkinteger(L, 1)));
    return 0;
}
static auto l_clip_paste(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_paste")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clipboard.access");
        return lua_error(L);
    }
    g_ctrl->pasteClip(static_cast<int>(luaL_checkinteger(L, 1)), static_cast<int>(luaL_checkinteger(L, 2)));
    return 0;
}
static auto l_clip_list(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("clip_list")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: clip.read");
        return lua_error(L);
    }
    QVariantList clips = g_ctrl->clips();
    lua_newtable(L);
    for (int i = 0; i < clips.size(); i++) {
        QVariantMap m = clips.value(i).toMap();
        lua_newtable(L);
        auto push = [&](const char *k, const QVariant &v) -> void {
            lua_pushstring(L, k);
            if (v.typeId() == QMetaType::Int || v.typeId() == QMetaType::LongLong) {
                lua_pushinteger(L, v.toInt());
            } else if (v.typeId() == QMetaType::Double || v.typeId() == QMetaType::Float) {
                lua_pushnumber(L, v.toDouble());
            } else {
                lua_pushstring(L, v.toString().toUtf8().constData());
            }
            lua_settable(L, -3);
        };
        push("id", m.value(QStringLiteral("id")));
        push("type", m.value(QStringLiteral("type")));
        push("layer", m.value(QStringLiteral("layer")));
        push("startFrame", m.value(QStringLiteral("startFrame")));
        push("duration", m.value(QStringLiteral("durationFrames")));
        lua_rawseti(L, -2, i + 1);
    }
    return 1;
}

// effect
static auto l_effect_add(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("effect_add")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: effect.modify");
        return lua_error(L);
    }
    g_ctrl->addEffect(static_cast<int>(luaL_checkinteger(L, 1)), QString::fromUtf8(luaL_checkstring(L, 2)));
    return 0;
}
static auto l_effect_remove(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("effect_remove")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: effect.modify");
        return lua_error(L);
    }
    g_ctrl->removeEffect(static_cast<int>(luaL_checkinteger(L, 1)), static_cast<int>(luaL_checkinteger(L, 2)));
    return 0;
}
static auto l_effect_set_param(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("effect_set_param")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: effect.modify");
        return lua_error(L);
    }
    // aviqtl_effect_set_param(clipId, effectIndex, paramName, value)
    int clipId = static_cast<int>(luaL_checkinteger(L, 1));
    int effectIndex = static_cast<int>(luaL_checkinteger(L, 2));
    const char *key = luaL_checkstring(L, 3);
    QVariant val;
    if (lua_type(L, 4) == LUA_TNUMBER) {
        val = lua_tonumber(L, 4);
    } else if (lua_type(L, 4) == LUA_TBOOLEAN) {
        val = static_cast<bool>(lua_toboolean(L, 4));
    } else {
        val = QString::fromUtf8(lua_tostring(L, 4));
    }
    g_ctrl->updateClipEffectParam(clipId, effectIndex, QString::fromUtf8(key), val);
    return 0;
}

// project
static auto l_project_get_width(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("project_width")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: project.read");
        return lua_error(L);
    }
    lua_pushinteger(L, g_ctrl->project()->width());
    return 1;
}
static auto l_project_get_height(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("project_height")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: project.read");
        return lua_error(L);
    }
    lua_pushinteger(L, g_ctrl->project()->height());
    return 1;
}
static auto l_project_get_fps(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("project_fps")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: project.read");
        return lua_error(L);
    }
    lua_pushnumber(L, g_ctrl->project()->fps());
    return 1;
}
static auto l_project_save(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("project_save")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: project.save");
        return lua_error(L);
    }
    bool ok = g_ctrl->saveProject(QString::fromUtf8(luaL_checkstring(L, 1)));
    lua_pushboolean(L, static_cast<int>(ok));
    return 1;
}
static auto l_project_load(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("project_load")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: project.load");
        return lua_error(L);
    }
    bool ok = g_ctrl->loadProject(QString::fromUtf8(luaL_checkstring(L, 1)));
    lua_pushboolean(L, static_cast<int>(ok));
    return 1;
}

// undo/redo
static auto l_undo(lua_State *L) -> int {
    _checkCtrl(L);
    // Undo/redo is allowed by default (no specific permission required)
    g_ctrl->undo();
    return 0;
}
static auto l_redo(lua_State *L) -> int {
    _checkCtrl(L);
    // Undo/redo is allowed by default (no specific permission required)
    g_ctrl->redo();
    return 0;
}

// settings
static auto l_settings_set(lua_State *L) -> int {
    if (!ModEngine::instance().checkPermission("settings_set")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: settings.write");
        return lua_error(L);
    }
    const char *key = luaL_checkstring(L, 1);
    const char *value = luaL_checkstring(L, 2);
    api_settings_set(key, value);
    return 0;
}
static auto l_settings_get(lua_State *L) -> int {
    if (!ModEngine::instance().checkPermission("settings_get")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: settings.read");
        return lua_error(L);
    }
    const char *key = luaL_checkstring(L, 1);
    const char *value = api_settings_get(key);
    lua_pushstring(L, value);
    return 1;
}

// scene
static auto l_scene_create(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("scene_create")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: scene.manage");
        return lua_error(L);
    }
    g_ctrl->createScene(QString::fromUtf8(luaL_checkstring(L, 1)));
    return 0;
}
static auto l_scene_remove(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("scene_remove")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: scene.manage");
        return lua_error(L);
    }
    g_ctrl->removeScene(static_cast<int>(luaL_checkinteger(L, 1)));
    return 0;
}
static auto l_scene_switch(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("scene_switch")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: scene.manage");
        return lua_error(L);
    }
    g_ctrl->switchScene(static_cast<int>(luaL_checkinteger(L, 1)));
    return 0;
}

// command
static auto l_command_begin_group(lua_State *L) -> int {
    _checkCtrl(L);
    const char *text = luaL_checkstring(L, 1);
    if (g_ctrl->timeline() != nullptr) {
        g_ctrl->timeline()->undoStack()->beginMacro(QString::fromUtf8(text));
    }
    return 0;
}
static auto l_command_end_group(lua_State *L) -> int {
    _checkCtrl(L);
    if (g_ctrl->timeline() != nullptr) {
        g_ctrl->timeline()->undoStack()->endMacro();
    }
    return 0;
}

auto ModEngine::instance() -> ModEngine & {
    static ModEngine inst;
    return inst;
}

ModEngine::~ModEngine() {
    if (L != nullptr) {
        lua_close(L);
    }
}

void ModEngine::initialize(void *ecsPtr) {
    if (L != nullptr) {
        return;
    }
    L = luaL_newstate();
    AviQtl::Scripting::LuaHost::setupSafeLuaState(L);

    // Register core pointer as global
    lua_pushlightuserdata(L, ecsPtr);
    lua_setglobal(L, "AVIQTL_CORE_PTR");

    _registerAviQtlAPI();
    lua_pushlightuserdata(L, &g_hostApi);
    lua_setglobal(L, "AVIQTL_HOST_API");
    m_apiRegistered = true;

    qInfo() << "[ModEngine] LuaJIT initialized. Core pointer registered as AVIQTL_CORE_PTR";
}

void ModEngine::registerController(void *controller) {
    g_ctrl = static_cast<AviQtl::UI::TimelineController *>(controller);
    if (L != nullptr && !m_apiRegistered) {
        _registerAviQtlAPI();
        m_apiRegistered = true;

        // Export Host API Table
        lua_pushlightuserdata(L, &g_hostApi);
        lua_setglobal(L, "AVIQTL_HOST_API");
    }
}

void ModEngine::_registerAviQtlAPI() {
    lua_register(L, "aviqtl_log", l_log);
    // transport
    lua_register(L, "aviqtl_transport_play", l_transport_play);
    lua_register(L, "aviqtl_transport_pause", l_transport_pause);
    lua_register(L, "aviqtl_transport_toggle", l_transport_toggle);
    lua_register(L, "aviqtl_transport_seek", l_transport_seek);
    lua_register(L, "aviqtl_transport_get_frame", l_transport_get_frame);
    lua_register(L, "aviqtl_transport_is_playing", l_transport_is_playing);
    // clip
    lua_register(L, "aviqtl_clip_create", l_clip_create);
    lua_register(L, "aviqtl_clip_delete", l_clip_delete);
    lua_register(L, "aviqtl_clip_update", l_clip_update);
    lua_register(L, "aviqtl_clip_select", l_clip_select);
    lua_register(L, "aviqtl_clip_split", l_clip_split);
    lua_register(L, "aviqtl_clip_copy", l_clip_copy);
    lua_register(L, "aviqtl_clip_cut", l_clip_cut);
    lua_register(L, "aviqtl_clip_paste", l_clip_paste);
    lua_register(L, "aviqtl_clip_list", l_clip_list);
    // effect
    lua_register(L, "aviqtl_effect_add", l_effect_add);
    lua_register(L, "aviqtl_effect_remove", l_effect_remove);
    lua_register(L, "aviqtl_effect_set_param", l_effect_set_param);
    // project
    lua_register(L, "aviqtl_project_width", l_project_get_width);
    lua_register(L, "aviqtl_project_height", l_project_get_height);
    lua_register(L, "aviqtl_project_fps", l_project_get_fps);
    lua_register(L, "aviqtl_project_save", l_project_save);
    lua_register(L, "aviqtl_project_load", l_project_load);
    // undo/redo
    lua_register(L, "aviqtl_undo", l_undo);
    lua_register(L, "aviqtl_redo", l_redo);
    // settings
    lua_register(L, "aviqtl_settings_set", l_settings_set);
    lua_register(L, "aviqtl_settings_get", l_settings_get);
    // scene
    lua_register(L, "aviqtl_scene_create", l_scene_create);
    lua_register(L, "aviqtl_scene_remove", l_scene_remove);
    lua_register(L, "aviqtl_scene_switch", l_scene_switch);
    // command
    lua_register(L, "aviqtl_command_begin_group", l_command_begin_group);
    lua_register(L, "aviqtl_command_end_group", l_command_end_group);

    // aviqtl.xxx() 形式のテーブルAPIをLua側で構築
    const char *aviqtl_table = R"(
aviqtl = {
    transport = {
        play       = aviqtl_transport_play,
        pause      = aviqtl_transport_pause,
        toggle     = aviqtl_transport_toggle,
        seek       = aviqtl_transport_seek,
        get_frame  = aviqtl_transport_get_frame,
        is_playing = aviqtl_transport_is_playing,
    },
    clip = {
        create = aviqtl_clip_create,
        delete = aviqtl_clip_delete,
        update = aviqtl_clip_update,
        select = aviqtl_clip_select,
        split  = aviqtl_clip_split,
        copy   = aviqtl_clip_copy,
        cut    = aviqtl_clip_cut,
        paste  = aviqtl_clip_paste,
        list   = aviqtl_clip_list,
    },
    effect = {
        add       = aviqtl_effect_add,
        remove    = aviqtl_effect_remove,
        set_param = aviqtl_effect_set_param,
    },
    project = {
        width        = aviqtl_project_width,
        height       = aviqtl_project_height,
        fps          = aviqtl_project_fps,
        save         = aviqtl_project_save,
        load         = aviqtl_project_load,
    },
    scene = {
        create = aviqtl_scene_create,
        remove = aviqtl_scene_remove,
        switch = aviqtl_scene_switch,
    },
    settings = {
        set = aviqtl_settings_set,
        get = aviqtl_settings_get,
    },
    command = {
        begin_group = aviqtl_command_begin_group,
        end_group = aviqtl_command_end_group,
    },
    log = aviqtl_log,
    undo = aviqtl_undo,
    redo = aviqtl_redo,
}
)";
    // Lua の delete/switch は予約語なので _G 経由でアクセスする場合のみ注意
    luaL_dostring(L, aviqtl_table);

    qInfo() << "[ModEngine] AviQtl Lua API registered.";
}

void ModEngine::loadPlugins() {
    // Ensure API is registered (registerController may have been called before initialize)
    if (!m_apiRegistered && L != nullptr) {
        _registerAviQtlAPI();
        lua_pushlightuserdata(L, &g_hostApi);
        lua_setglobal(L, "AVIQTL_HOST_API");
        m_apiRegistered = true;
    }

    QString pluginsPath = QCoreApplication::applicationDirPath() + QLatin1String("/plugins");
    QDir dir(pluginsPath);

    if (!dir.exists()) {
        dir.mkpath(QStringLiteral("."));
        return;
    }

    // First pass: load manifests
    const QStringList subdirs = dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot);
    for (const QString &subdir : subdirs) {
        QString pluginDir = pluginsPath + QStringLiteral("/") + subdir;
        PluginManifest manifest = loadManifest(pluginDir);
        if (manifest.isValid()) {
            qInfo() << "[ModEngine] Found plugin:" << manifest.name << "v" << manifest.version << "(" << manifest.id << ")";
            m_loadedPlugins.append(manifest);
        }
    }

    // Second pass: load Lua files (both from root and subdirectories)
    QStringList filters;
    filters << QStringLiteral("*.lua");
    QFileInfoList files = dir.entryInfoList(filters, QDir::Files, QDir::Name);

    for (const QFileInfo &fileInfo : files) {
        qInfo() << "[ModEngine] Loading MOD:" << fileInfo.fileName();

        // Parse script parameters from header
        ScriptMetadata scriptMeta = loadScriptParams(fileInfo.absoluteFilePath());
        if (!scriptMeta.params.isEmpty()) {
            qInfo() << "[ModEngine] Found" << scriptMeta.params.size() << "parameters in" << fileInfo.fileName();
        }

        // Create PluginInfo for single-file plugins
        PluginInfo info;
        info.filePath = fileInfo.absoluteFilePath();
        info.scriptMeta = scriptMeta;

        // Load saved parameter values
        for (const ScriptParam &param : scriptMeta.params) {
            QString settingsKey = QStringLiteral("plugin_param.single.%1.%2").arg(fileInfo.fileName(), param.varName);
            QVariant saved = AviQtl::Core::SettingsManager::instance().value(settingsKey);
            if (saved.isValid()) {
                info.paramValues[param.varName] = saved;
            } else {
                info.paramValues[param.varName] = param.defaultValue;
            }
        }

        m_pluginInfos.append(info);

        // Inject parameters before loading
        injectPluginParams(L, info);

        if (luaL_dofile(L, fileInfo.absoluteFilePath().toUtf8().constData())) {
            qCritical() << "[ModEngine] Load Error:" << lua_tostring(L, -1);
            lua_pop(L, 1);
        }
    }

    // Load from subdirectories that have main.lua
    for (const QString &subdir : subdirs) {
        QString mainLua = pluginsPath + QStringLiteral("/") + subdir + QStringLiteral("/main.lua");
        if (QFile::exists(mainLua)) {
            qInfo() << "[ModEngine] Loading plugin:" << subdir;

            // Parse script parameters from main.lua
            ScriptMetadata scriptMeta = loadScriptParams(mainLua);

            // Find the plugin manifest to get the ID for permission checking
            PluginInfo info;
            info.filePath = mainLua;
            info.scriptMeta = scriptMeta;

            QString manifestDir = pluginsPath + QStringLiteral("/") + subdir;
            PluginManifest m = loadManifest(manifestDir);
            if (m.isValid()) {
                info.manifest = m;
                setCurrentPluginId(m.id);
                m_lastLoadedPluginId = m.id;

                // Load saved parameter values
                for (const ScriptParam &param : scriptMeta.params) {
                    QString settingsKey = QStringLiteral("plugin_param.%1.%2").arg(m.id, param.varName);
                    QVariant saved = AviQtl::Core::SettingsManager::instance().value(settingsKey);
                    if (saved.isValid()) {
                        info.paramValues[param.varName] = saved;
                    } else {
                        info.paramValues[param.varName] = param.defaultValue;
                    }
                }
            }

            m_pluginInfos.append(info);

            // Inject parameters before loading
            injectPluginParams(L, info);

            if (luaL_dofile(L, mainLua.toUtf8().constData())) {
                qCritical() << "[ModEngine] Plugin Error:" << lua_tostring(L, -1);
                lua_pop(L, 1);
            }
            setCurrentPluginId(QString()); // Clear after loading
        }
    }
}

PluginManifest ModEngine::loadManifest(const QString &pluginDir) {
    PluginManifest manifest;
    QString manifestPath = pluginDir + QStringLiteral("/manifest.lua");

    if (!QFile::exists(manifestPath)) {
        return manifest;
    }

    // Use existing Lua state or create a temporary one for parsing
    bool tempLua = (L == nullptr);
    lua_State *ls = L;
    if (tempLua) {
        ls = luaL_newstate();
        if (!ls) return manifest;
    }

    // Load and execute manifest.lua to get the manifest table
    if (luaL_dofile(ls, manifestPath.toUtf8().constData()) != LUA_OK) {
        qWarning() << "[ModEngine] Failed to load manifest:" << lua_tostring(ls, -1);
        lua_pop(ls, 1);
        if (tempLua) lua_close(ls);
        return manifest;
    }

    // Get the returned table
    if (!lua_istable(ls, -1)) {
        qWarning() << "[ModEngine] Manifest must return a table";
        lua_pop(ls, 1);
        if (tempLua) lua_close(ls);
        return manifest;
    }

    // Extract fields
    auto getString = [&](const char *key) -> QString {
        lua_getfield(ls, -1, key);
        const char *val = lua_tostring(ls, -1);
        QString result = val ? QString::fromUtf8(val) : QString();
        lua_pop(ls, 1);
        return result;
    };

    manifest.id = getString("id");
    manifest.name = getString("name");
    manifest.version = getString("version");
    manifest.author = getString("author");
    manifest.description = getString("description");
    manifest.minAppVersion = getString("min_app_version");

    lua_pop(ls, 1); // Pop the table
    if (tempLua) lua_close(ls);
    return manifest;
}

void ModEngine::unloadPlugins() {
    // Note: Lua doesn't have a built-in unload mechanism
    // We would need to track loaded chunks and their globals
    // For now, this just clears the manifest list
    m_loadedPlugins.clear();
    qInfo() << "[ModEngine] Plugin list cleared (full unload requires Lua state reset)";
}

void ModEngine::enableHotReload(bool enable) {
    if (m_hotReloadEnabled == enable) {
        return;
    }

    m_hotReloadEnabled = enable;

    if (enable) {
        _setupFileWatcher();
        qInfo() << "[ModEngine] Hot reload enabled";
    } else {
        if (m_fileWatcher) {
            m_fileWatcher->clearPaths();
        }
        qInfo() << "[ModEngine] Hot reload disabled";
    }
}

void ModEngine::_setupFileWatcher() {
    if (m_fileWatcher) {
        m_fileWatcher->clearPaths();
    } else {
        m_fileWatcher = new PluginFileWatcher();
    }

    QString pluginsPath = QCoreApplication::applicationDirPath() + QLatin1String("/plugins");

    // Watch the plugins directory
    if (QDir(pluginsPath).exists()) {
        m_fileWatcher->watchPath(pluginsPath);

        // Also watch subdirectories
        const QStringList subdirs = QDir(pluginsPath).entryList(QDir::Dirs | QDir::NoDotAndDotDot);
        for (const QString &subdir : subdirs) {
            m_fileWatcher->watchPath(pluginsPath + QStringLiteral("/") + subdir);
        }
    }

    QObject::connect(m_fileWatcher, &PluginFileWatcher::directoryChanged, [this](const QString &path) {
        qInfo() << "[ModEngine] Plugin directory changed:" << path;
        _onPluginDirectoryChanged(path);
    });
}

void ModEngine::_onPluginDirectoryChanged(const QString &path) {
    qInfo() << "[ModEngine] Plugin directory changed:" << path;

    // Simple approach: reload all plugins
    // A more sophisticated approach would track which plugin changed
    // and only reload that specific plugin

    // Clear current plugins
    m_loadedPlugins.clear();
    m_pluginInfos.clear();
    m_lastLoadedPluginId.clear();

    // Reload all plugins
    loadPlugins();

    // Re-call onLoad hook
    onLoad();

    qInfo() << "[ModEngine] Plugins reloaded due to file changes";
}

void ModEngine::onUpdate() {
    if (L == nullptr) {
        return;
    }
    _callHook("AviQtlUpdateHook");
}

void ModEngine::onLoad() {
    if (L == nullptr) {
        return;
    }
    _callHook("AviQtlOnLoad");
    qInfo() << "[ModEngine] onLoad hook called";
}

void ModEngine::onUnload() {
    if (L == nullptr) {
        return;
    }
    _callHook("AviQtlOnUnload");
    qInfo() << "[ModEngine] onUnload hook called";
}

void ModEngine::onProjectOpen(const QString &path) {
    if (L == nullptr) {
        return;
    }
    lua_pushstring(L, path.toUtf8().constData());
    _callHook("AviQtlOnProjectOpen", 1);
    qInfo() << "[ModEngine] onProjectOpen hook called:" << path;
}

void ModEngine::onProjectSave(const QString &path) {
    if (L == nullptr) {
        return;
    }
    lua_pushstring(L, path.toUtf8().constData());
    _callHook("AviQtlOnProjectSave", 1);
    qInfo() << "[ModEngine] onProjectSave hook called:" << path;
}

void ModEngine::onClipChange() {
    if (L == nullptr) {
        return;
    }
    _callHook("AviQtlOnClipChange");
}

void ModEngine::_callHook(const char *hookName, int nargs) {
    // Stack before: [arg1, ..., argN]
    lua_getglobal(L, hookName);
    // Stack after: [arg1, ..., argN, function]
    if (!lua_isfunction(L, -1)) {
        lua_pop(L, 1 + nargs);
        return;
    }
    // Move function below arguments: [arg1, ..., argN, function] -> [function, arg1, ..., argN]
    lua_insert(L, -(nargs + 1));
    // Set plugin context for permission checks during hook execution
    QString prevPluginId = m_currentPluginId;
    if (m_currentPluginId.isEmpty() && !m_lastLoadedPluginId.isEmpty()) {
        m_currentPluginId = m_lastLoadedPluginId;
    }
    if (lua_pcall(L, nargs, 0, 0) != 0) {
        qCritical() << "[ModEngine] Hook" << hookName << "Error:" << lua_tostring(L, -1);
        lua_pop(L, 1);
    }
    m_currentPluginId = prevPluginId;
}

ScriptMetadata ModEngine::loadScriptParams(const QString &scriptPath) {
    QFile file(scriptPath);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        return ScriptMetadata();
    }

    // Read first 100 lines for header parsing
    QTextStream in(&file);
    QStringList lines;
    for (int i = 0; i < 100 && !in.atEnd(); ++i) {
        lines.append(in.readLine());
    }
    file.close();

    return ScriptParamParser::parseHeader(lines);
}

QVariantMap ModEngine::getPluginParams(const QString &pluginId) const {
    for (const PluginInfo &info : m_pluginInfos) {
        if (info.manifest.id == pluginId) {
            return info.paramValues;
        }
    }
    return QVariantMap();
}

void ModEngine::setPluginParam(const QString &pluginId, const QString &key, const QVariant &value) {
    for (PluginInfo &info : m_pluginInfos) {
        if (info.manifest.id == pluginId) {
            info.paramValues[key] = value;
            // Save to settings for persistence
            QString settingsKey = QStringLiteral("plugin_param.%1.%2").arg(pluginId, key);
            AviQtl::Core::SettingsManager::instance().setValue(settingsKey, value);
            return;
        }
    }
}

void ModEngine::injectPluginParams(lua_State *L, const PluginInfo &info) {
    // Inject parameter values as global Lua variables
    for (const ScriptParam &param : info.scriptMeta.params) {
        QVariant value = info.paramValues.value(param.varName, param.defaultValue);
        if (param.type == ScriptParamType::Select && value.typeId() == QMetaType::QString) {
            const QString text = value.toString();
            for (const ScriptParamOption &option : param.options) {
                if (text == option.label || text == option.value.toString()) {
                    value = option.value;
                    break;
                }
            }
        }

        switch (value.typeId()) {
        case QMetaType::Bool:
            lua_pushboolean(L, value.toBool() ? 1 : 0);
            break;
        case QMetaType::Int:
        case QMetaType::LongLong:
            lua_pushinteger(L, value.toLongLong());
            break;
        case QMetaType::Double:
        case QMetaType::Float:
            lua_pushnumber(L, value.toDouble());
            break;
        case QMetaType::QString:
            lua_pushstring(L, value.toString().toUtf8().constData());
            break;
        default:
            // For colors and other types, convert to number
            lua_pushnumber(L, value.toDouble());
            break;
        }

        lua_setglobal(L, param.varName.toUtf8().constData());
    }
}

} // namespace AviQtl::Scripting
