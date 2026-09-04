#include "mod_engine.hpp"
#include "../core/include/bounded_file.hpp"
#include "../core/include/permission_manager.hpp"
#include "../core/include/rust_plugin_document.hpp"
#include "../core/include/settings_manager.hpp"
#include "../core/include/version.hpp"
#include "../ui/include/timeline_controller.hpp"
#include "lua_host.hpp"
#include <QCoreApplication>
#include <QDebug>
#include <QPointer>
#include <QScopeGuard>
#include <QStringList>
#include <QVariant>
#include <array>
#include <utility>

namespace AviQtl::Scripting {

// Lua から参照できるグローバルポインタ (QPointer で lifetime 安全に監視)
static QPointer<AviQtl::UI::TimelineController> g_ctrl;

static QVariantMap manifestToVariantMap(const PluginManifest &manifest) {
    return {
        {QStringLiteral("id"), manifest.id},
        {QStringLiteral("name"), manifest.name},
        {QStringLiteral("version"), manifest.version},
        {QStringLiteral("author"), manifest.author},
        {QStringLiteral("description"), manifest.description},
        {QStringLiteral("minAppVersion"), manifest.minAppVersion},
    };
}

static PluginManifest manifestFromVariantMap(const QVariantMap &manifest) {
    return {
        manifest.value(QStringLiteral("id")).toString(),
        manifest.value(QStringLiteral("name")).toString(),
        manifest.value(QStringLiteral("version")).toString(),
        manifest.value(QStringLiteral("author")).toString(),
        manifest.value(QStringLiteral("description")).toString(),
        manifest.value(QStringLiteral("minAppVersion")).toString(),
    };
}

static QString singleFilePluginId(const QFileInfo &fileInfo) {
    // Keep the existing identity format because permissions and parameters are persisted by ID.
    return QStringLiteral("file:%1").arg(fileInfo.fileName());
}

static QString pluginPathIdentity(const QString &path) {
    const QFileInfo info(path);
    const QString canonicalPath = info.canonicalFilePath();
    return canonicalPath.isEmpty() ? info.absoluteFilePath() : canonicalPath;
}

bool PluginManifest::isValid() const {
    const auto normalized = AviQtl::RustCore::Plugin::normalizeManifest(manifestToVariantMap(*this));
    return normalized.has_value() && normalized->valid;
}

static constexpr std::array<const char *, 6> kPluginHookNames = {
    "AviQtlUpdateHook", "AviQtlOnLoad", "AviQtlOnUnload", "AviQtlOnProjectOpen", "AviQtlOnProjectSave", "AviQtlOnClipChange",
};

// C API Wrappers (used by Lua bindings)
extern "C" {
static void api_log(const char *msg) {
    if (!ModEngine::instance().checkPermission("log")) {
        return; // Silently deny log output
    }
    if (g_ctrl != nullptr) {
        AviQtl::UI::TimelineController::log(QString::fromUtf8(msg));
    }
}

// Settings API (scoped to current plugin)
static thread_local char g_settings_buf[4096]; // Buffer for returning strings (thread_local for safety)
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

bool ModEngine::checkPermission(const char *apiName) const {
    if (m_currentPluginId.isEmpty()) {
        return true; // No plugin context, allow (for direct calls)
    }
    return AviQtl::Core::PermissionManager::instance().hasApiPermission(m_currentPluginId, apiName);
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
    if (!ModEngine::instance().checkPermission("undo")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: history.control");
        return lua_error(L);
    }
    g_ctrl->undo();
    return 0;
}
static auto l_redo(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("redo")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: history.control");
        return lua_error(L);
    }
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
    if (!ModEngine::instance().checkPermission("command_begin_group")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: history.control");
        return lua_error(L);
    }
    const char *text = luaL_checkstring(L, 1);
    if (g_ctrl->timeline() != nullptr) {
        g_ctrl->timeline()->undoStack()->beginMacro(QString::fromUtf8(text));
    }
    return 0;
}
static auto l_command_end_group(lua_State *L) -> int {
    _checkCtrl(L);
    if (!ModEngine::instance().checkPermission("command_end_group")) {
        lua_pushstring(L, "[AviQtlAPI] Permission denied: history.control");
        return lua_error(L);
    }
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
    releasePluginHooks();
    if (L != nullptr) {
        lua_close(L);
    }
}

void ModEngine::initialize() {
    m_initialized = true;
    if (L != nullptr) {
        return;
    }
    L = luaL_newstate();
    if (L == nullptr) {
        qCritical() << "[ModEngine] Failed to create Lua state.";
        return;
    }
    AviQtl::Scripting::LuaHost::setupSafeLuaState(L);

    registerAviQtlAPI();
    m_apiRegistered = true;

    qInfo() << "[ModEngine] LuaJIT initialized";
}

void ModEngine::resetLuaState() {
    releasePluginHooks();
    if (L != nullptr) {
        lua_close(L);
        L = nullptr;
    }
    m_apiRegistered = false;
    if (m_initialized) {
        initialize();
    }
}

void ModEngine::registerController(AviQtl::UI::TimelineController *controller) {
    QObject::disconnect(m_clipChangeConnection);
    g_ctrl = controller;
    if (controller != nullptr && controller->timeline() != nullptr) {
        m_clipChangeConnection = QObject::connect(controller->timeline(), &AviQtl::UI::TimelineService::clipsChanged, [this]() { onClipChange(); });
    }
    if (L != nullptr && !m_apiRegistered) {
        registerAviQtlAPI();
        m_apiRegistered = true;
    }
}

void ModEngine::registerAviQtlAPI() {
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
    if (L == nullptr) {
        initialize();
    }
    if (L == nullptr) {
        return;
    }

    // Ensure API is registered (registerController may have been called before initialize)
    if (!m_apiRegistered && L != nullptr) {
        registerAviQtlAPI();
        m_apiRegistered = true;
    }

    QString pluginsPath = QCoreApplication::applicationDirPath() + QLatin1String("/plugins");
    QDir dir(pluginsPath);

    if (!dir.exists()) {
        dir.mkpath(QStringLiteral("."));
        return;
    }

    const QStringList subdirs = dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot);
    QStringList filters;
    filters << QStringLiteral("*.lua");
    QFileInfoList files = dir.entryInfoList(filters, QDir::Files, QDir::Name);

    for (const QFileInfo &fileInfo : files) {
        loadSingleFilePlugin(fileInfo);
    }

    // Load from subdirectories that have main.lua
    for (const QString &subdir : subdirs) {
        loadDirectoryPlugin(subdir, pluginsPath);
    }
}

void ModEngine::loadSingleFilePlugin(const QFileInfo &fileInfo) {
    qInfo() << "[ModEngine] Loading MOD:" << fileInfo.fileName();
    PluginManifest manifest;
    manifest.id = singleFilePluginId(fileInfo);
    manifest.name = fileInfo.completeBaseName();
    manifest.version = QStringLiteral("file");
    loadPlugin(manifest, fileInfo.absoluteFilePath(), true);
}

void ModEngine::loadDirectoryPlugin(const QString &subdir, const QString &pluginsPath) {
    QString mainLua = pluginsPath + QStringLiteral("/") + subdir + QStringLiteral("/main.lua");
    if (!QFile::exists(mainLua)) {
        return;
    }

    qInfo() << "[ModEngine] Loading plugin:" << subdir;

    QString manifestDir = pluginsPath + QStringLiteral("/") + subdir;
    loadPlugin(loadManifest(manifestDir), mainLua, false);
}

std::optional<PluginManifest> ModEngine::validatePlugin(const PluginManifest &manifest,
                                                        const QString &scriptPath,
                                                        bool singleFile) const {
    const QString pathIdentity = pluginPathIdentity(scriptPath);
    QVariantList loaded;
    loaded.reserve(m_pluginInfos.size());
    for (const PluginInfo &info : m_pluginInfos) {
        loaded.append(QVariantMap{
            {QStringLiteral("id"), info.manifest.id},
            {QStringLiteral("path"), info.filePath},
        });
    }
    const auto validation = AviQtl::RustCore::Plugin::validateManifest(
        manifestToVariantMap(manifest), singleFile,
        singleFile ? singleFilePluginId(QFileInfo(scriptPath)) : QString(),
        QString::fromUtf8(AviQtl::VERSION_STRING), pathIdentity, loaded);
    if (!validation.has_value() || validation->status == QStringLiteral("invalid_manifest")) {
        qWarning() << "[ModEngine] Skipping plugin with an invalid manifest:" << scriptPath;
        return std::nullopt;
    }
    const PluginManifest normalized = manifestFromVariantMap(validation->manifest);
    if (validation->status == QStringLiteral("invalid_id")) {
        qWarning() << "[ModEngine] Skipping plugin with an invalid ID:" << normalized.id;
        return std::nullopt;
    }
    if (validation->status == QStringLiteral("requires_newer_app")) {
        qWarning() << "[ModEngine] Skipping plugin" << scriptPath << ": requires AviQtl"
                   << normalized.minAppVersion << "or newer (current:"
                   << QString::fromUtf8(AviQtl::VERSION_STRING) << ")";
        return std::nullopt;
    }
    if (validation->status == QStringLiteral("duplicate")) {
        qWarning() << "[ModEngine] Skipping duplicate plugin ID:" << normalized.id;
        return std::nullopt;
    }
    if (validation->status != QStringLiteral("ok")) {
        qWarning() << "[ModEngine] Skipping plugin after manifest validation:" << scriptPath;
        return std::nullopt;
    }
    return normalized;
}

bool ModEngine::loadPlugin(const PluginManifest &candidate, const QString &scriptPath, bool singleFile) {
    const auto validated = validatePlugin(candidate, scriptPath, singleFile);
    if (!validated.has_value())
        return false;
    const PluginManifest &manifest = *validated;

    QString readError;
    const auto script = AviQtl::Core::Internal::readFileBounded(
        scriptPath, AviQtl::Core::Internal::FileSizeLimit::PluginScript, &readError);
    if (!script.has_value()) {
        qWarning().noquote() << QStringLiteral("[ModEngine] Skipping unreadable plugin:") << scriptPath << readError;
        return false;
    }

    PluginInfo info;
    info.manifest = manifest;
    info.filePath = pluginPathIdentity(scriptPath);
    info.scriptMeta = ScriptParamParser::parse(QString::fromUtf8(*script));
    for (const ScriptParam &param : std::as_const(info.scriptMeta.params)) {
        const QString settingsKey = QStringLiteral("plugin_param.%1.%2").arg(manifest.id, param.varName);
        QVariant saved = AviQtl::Core::SettingsManager::instance().value(settingsKey);
        if (!saved.isValid() && singleFile) {
            const QString legacyKey = QStringLiteral("plugin_param.single.%1.%2")
                                          .arg(QFileInfo(scriptPath).fileName(), param.varName);
            saved = AviQtl::Core::SettingsManager::instance().value(legacyKey);
        }
        info.paramValues[param.varName] = saved.isValid() ? saved : param.defaultValue;
    }

    const QString previousPluginId = m_currentPluginId;
    m_currentPluginId = manifest.id;
    const auto pluginGuard = qScopeGuard([this, previousPluginId]() {
        LuaHost::clearInstructionLimit(L);
        m_currentPluginId = previousPluginId;
    });
    clearHookGlobals();
    injectPluginParams(L, info);

    const QByteArray chunkName = QFile::encodeName(QStringLiteral("@") + info.filePath);
    LuaHost::installInstructionLimit(L);
    int loadStatus = luaL_loadbuffer(L, script->constData(), static_cast<size_t>(script->size()), chunkName.constData());
    if (loadStatus == LUA_OK)
        loadStatus = lua_pcall(L, 0, 0, 0);
    if (loadStatus != LUA_OK) {
        qCritical() << "[ModEngine] Plugin Error:" << lua_tostring(L, -1);
        lua_pop(L, 1);
        clearHookGlobals();
        return false;
    }

    capturePluginHooks(manifest.id);
    m_pluginInfos.append(info);
    m_loadedPlugins.append(manifest);
    qInfo() << "[ModEngine] Loaded plugin:" << manifest.name << "v" << manifest.version << "(" << manifest.id << ")";
    return true;
}

PluginManifest ModEngine::loadManifest(const QString &pluginDir) {
    PluginManifest manifest;
    QString manifestPath = pluginDir + QStringLiteral("/manifest.lua");

    if (!QFile::exists(manifestPath)) {
        return manifest;
    }

    QString readError;
    const auto script = AviQtl::Core::Internal::readFileBounded(
        manifestPath, AviQtl::Core::Internal::FileSizeLimit::PluginManifest, &readError);
    if (!script.has_value()) {
        qWarning().noquote() << QStringLiteral("[ModEngine] Failed to read manifest:") << manifestPath << readError;
        return manifest;
    }

    // Always use an isolated Lua state for manifest parsing to prevent
    // manifest.lua from calling registered AviQtl APIs or accessing main state
    lua_State *ls = luaL_newstate();
    if (ls == nullptr) {
        return manifest;
    }

    LuaHost::setupSafeLuaState(ls);
    LuaHost::installInstructionLimit(ls);
    const QByteArray chunkName = QFile::encodeName(QStringLiteral("@") + manifestPath);
    int loadStatus = luaL_loadbuffer(ls, script->constData(), static_cast<size_t>(script->size()), chunkName.constData());
    if (loadStatus == LUA_OK)
        loadStatus = lua_pcall(ls, 0, 1, 0);
    LuaHost::clearInstructionLimit(ls);
    if (loadStatus != LUA_OK) {
        qWarning() << "[ModEngine] Failed to load manifest:" << lua_tostring(ls, -1);
        lua_pop(ls, 1);
        lua_close(ls);
        return manifest;
    }

    // Get the returned table
    if (!lua_istable(ls, -1)) {
        qWarning() << "[ModEngine] Manifest must return a table";
        lua_pop(ls, 1);
        lua_close(ls);
        return manifest;
    }

    // Extract fields
    auto getString = [&](const char *key) -> QString {
        lua_getfield(ls, -1, key);
        const char *val = lua_type(ls, -1) == LUA_TSTRING ? lua_tostring(ls, -1) : nullptr;
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

    const auto normalized =
        AviQtl::RustCore::Plugin::normalizeManifest(manifestToVariantMap(manifest));
    if (normalized.has_value())
        manifest = manifestFromVariantMap(normalized->manifest);

    lua_pop(ls, 1); // Pop the table
    lua_close(ls);
    return manifest;
}

void ModEngine::unloadPlugins() {
    if (!m_pluginRuntimes.isEmpty()) {
        onUnload();
    }
    m_loadedPlugins.clear();
    m_pluginInfos.clear();
    m_currentPluginId.clear();
    resetLuaState();
    qInfo() << "[ModEngine] Plugins unloaded and Lua state reset";
}

void ModEngine::enableHotReload(bool enable) {
    if (m_hotReloadEnabled == enable) {
        return;
    }

    m_hotReloadEnabled = enable;

    if (enable) {
        setupFileWatcher();
        qInfo() << "[ModEngine] Hot reload enabled";
    } else {
        if (m_fileWatcher) {
            m_fileWatcher->clearPaths();
        }
        qInfo() << "[ModEngine] Hot reload disabled";
    }
}

void ModEngine::setupFileWatcher() {
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

    static bool watcherConnectionsInitialized = false;
    if (!watcherConnectionsInitialized) {
        QObject::connect(m_fileWatcher, &PluginFileWatcher::directoryChanged, [this](const QString &path) {
            qInfo() << "[ModEngine] Plugin directory changed:" << path;
            onPluginDirectoryChanged(path);
        });

        m_reloadDebounceTimer.setSingleShot(true);
        m_reloadDebounceTimer.setInterval(500);
        QObject::connect(&m_reloadDebounceTimer, &QTimer::timeout, [this]() {
            unloadPlugins();
            loadPlugins();
            onLoad();
            qInfo() << "[ModEngine] Plugins reloaded due to file changes";
        });
        watcherConnectionsInitialized = true;
    }
}

void ModEngine::onPluginDirectoryChanged(const QString &path) {
    qInfo() << "[ModEngine] Plugin directory changed:" << path;
    // Debounce: delay reload to coalesce rapid file change events
    m_reloadDebounceTimer.start(500);
}

void ModEngine::onUpdate() {
    if (L == nullptr) {
        return;
    }
    callHooks("AviQtlUpdateHook");
}

void ModEngine::onLoad() {
    if (L == nullptr) {
        return;
    }
    callHooks("AviQtlOnLoad");
    qInfo() << "[ModEngine] onLoad hook called";
}

void ModEngine::onUnload() {
    if (L == nullptr) {
        return;
    }
    callHooks("AviQtlOnUnload");
    qInfo() << "[ModEngine] onUnload hook called";
}

void ModEngine::onProjectOpen(const QString &path) {
    if (L == nullptr) {
        return;
    }
    callHooks("AviQtlOnProjectOpen", &path);
    qInfo() << "[ModEngine] onProjectOpen hook called:" << path;
}

void ModEngine::onProjectSave(const QString &path) {
    if (L == nullptr) {
        return;
    }
    callHooks("AviQtlOnProjectSave", &path);
    qInfo() << "[ModEngine] onProjectSave hook called:" << path;
}

void ModEngine::onClipChange() {
    if (L == nullptr) {
        return;
    }
    callHooks("AviQtlOnClipChange");
}

void ModEngine::clearHookGlobals() {
    if (L == nullptr) {
        return;
    }
    for (const char *hookName : kPluginHookNames) {
        lua_pushnil(L);
        lua_setglobal(L, hookName);
    }
}

void ModEngine::capturePluginHooks(const QString &pluginId) {
    PluginRuntime runtime;
    runtime.pluginId = pluginId;
    for (const char *hookName : kPluginHookNames) {
        lua_getglobal(L, hookName);
        if (lua_isfunction(L, -1)) {
            runtime.hookRefs.insert(QByteArray(hookName), luaL_ref(L, LUA_REGISTRYINDEX));
        } else {
            lua_pop(L, 1);
        }
        lua_pushnil(L);
        lua_setglobal(L, hookName);
    }
    if (!runtime.hookRefs.isEmpty()) {
        m_pluginRuntimes.append(std::move(runtime));
    }
}

void ModEngine::releasePluginHooks() {
    if (L != nullptr) {
        for (const PluginRuntime &runtime : std::as_const(m_pluginRuntimes)) {
            for (int reference : runtime.hookRefs) {
                luaL_unref(L, LUA_REGISTRYINDEX, reference);
            }
        }
    }
    m_pluginRuntimes.clear();
}

void ModEngine::callHooks(const char *hookName, const QString *argument) {
    if (L == nullptr || m_dispatchingHooks) {
        return;
    }
    m_dispatchingHooks = true;
    const auto dispatchGuard = qScopeGuard([this]() { m_dispatchingHooks = false; });
    for (const PluginRuntime &runtime : std::as_const(m_pluginRuntimes)) {
        const auto refIt = runtime.hookRefs.constFind(QByteArray(hookName));
        if (refIt == runtime.hookRefs.cend()) {
            continue;
        }

        lua_rawgeti(L, LUA_REGISTRYINDEX, refIt.value());
        int argumentCount = 0;
        if (argument != nullptr) {
            const QByteArray encoded = argument->toUtf8();
            lua_pushlstring(L, encoded.constData(), static_cast<size_t>(encoded.size()));
            argumentCount = 1;
        }

        const QString previousPluginId = m_currentPluginId;
        m_currentPluginId = runtime.pluginId;
        LuaHost::installInstructionLimit(L);
        const int hookStatus = lua_pcall(L, argumentCount, 0, 0);
        LuaHost::clearInstructionLimit(L);
        if (hookStatus != LUA_OK) {
            qCritical() << "[ModEngine] Hook" << hookName << "for plugin" << (runtime.pluginId.isEmpty() ? QStringLiteral("<legacy>") : runtime.pluginId) << "failed:" << lua_tostring(L, -1);
            lua_pop(L, 1);
        }
        m_currentPluginId = previousPluginId;
    }
}

ScriptMetadata ModEngine::loadScriptParams(const QString &scriptPath) {
    const auto script = AviQtl::Core::Internal::readFileBounded(
        scriptPath, AviQtl::Core::Internal::FileSizeLimit::PluginScript);
    if (!script.has_value()) {
        return ScriptMetadata();
    }
    return ScriptParamParser::parse(QString::fromUtf8(*script));
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
