#include "lua_host.hpp"
#include <QDebug>
#include <cmath>
#include <iostream>
#include <lua.hpp> // Standard Lua/LuaJIT header
#include <unordered_map>
#include <vector>

namespace AviQtl::Scripting {

auto LuaHost::instance() -> LuaHost & {
    static LuaHost inst;
    return inst;
}

LuaHost::LuaHost() : L(nullptr) {
    // メインスレッド用インスタンスの初期化（後方互換性のため）
    // 実際の評価はスレッドローカルなステートを使用する
    initialize();
}

LuaHost::~LuaHost() {
    if (L != nullptr) {
        lua_close(L);
        L = nullptr;
    }
}

void LuaHost::setupSafeLuaState(lua_State *L) {
    // Only open safe libraries — never expose io, os, debug, ffi, package
    static constexpr struct { const char *name; lua_CFunction func; } safeLibs[] = {
        {"",         luaopen_base},
        {"math",     luaopen_math},
        {"string",   luaopen_string},
        {"table",    luaopen_table},
        {"bit",      luaopen_bit},
    };
    for (const auto &lib : safeLibs) {
        lua_pushcfunction(L, lib.func);
        lua_pushstring(L, lib.name);
        lua_call(L, 1, 1);
        lua_pop(L, 1);
    }

    // math library shortcut
    lua_getglobal(L, "math");
    lua_setglobal(L, "m");

    // Remove dangerous base library functions that bypass the "return <expr>" sandbox
    lua_pushnil(L);
    lua_setglobal(L, "loadstring");
    lua_pushnil(L);
    lua_setglobal(L, "load");
    lua_pushnil(L);
    lua_setglobal(L, "dofile");
    lua_pushnil(L);
    lua_setglobal(L, "loadfile");

    const char *math_shortcuts = "sin = math.sin; cos = math.cos; tan = math.tan; "
                                 "abs = math.abs; max = math.max; min = math.min; "
                                 "floor = math.floor; ceil = math.ceil; pi = math.pi; "
                                 "random = math.random;";

    if (luaL_dostring(L, math_shortcuts) != LUA_OK) {
        qWarning() << "[LuaHost] Thread-local Lua state init failed:" << lua_tostring(L, -1);
        lua_pop(L, 1);
    }
}

void LuaHost::initialize() {
    if (L != nullptr) {
        lua_close(L);
    }
    L = luaL_newstate();
    setupSafeLuaState(L);
    qDebug() << "[LuaHost] LuaJIT engine initialized (Main Thread)";
}

struct ThreadLocalLua {
    ThreadLocalLua(const ThreadLocalLua &) = delete;
    auto operator=(const ThreadLocalLua &) -> ThreadLocalLua & = delete;

    lua_State *state = nullptr;
    std::unordered_map<std::string, int> compiledRegistry; // スクリプト文字列 -> Registry Index

    ThreadLocalLua() : state(luaL_newstate()) {

        if (state != nullptr) {
            LuaHost::setupSafeLuaState(state);
        }
    }

    ~ThreadLocalLua() {
        if (state != nullptr) {
            for (auto const &[expr, ref] : compiledRegistry) {
                luaL_unref(state, LUA_REGISTRYINDEX, ref);
            }
        }
        if (state != nullptr) {
            lua_close(state);
            state = nullptr;
        }
    } // NOLINT(bugprone-easily-swappable-parameters)
};

auto LuaHost::evaluate(const std::string &expression, double time, int index, double currentValue) -> double {
    // スレッドごとに独立したLuaステートを使用することでロックフリー化を実現
    thread_local ThreadLocalLua t_lua;
    lua_State *threadL = t_lua.state;

    if (threadL == nullptr) {
        return currentValue;
    }

    // Reset stack
    lua_settop(threadL, 0);

    lua_pushnumber(threadL, time);
    lua_setglobal(threadL, "time");

    lua_pushnumber(threadL, time); // エイリアス 't'
    lua_setglobal(threadL, "t");

    lua_pushinteger(threadL, index);
    lua_setglobal(threadL, "index");

    lua_pushnumber(threadL, currentValue);
    lua_setglobal(threadL, "value");

    lua_pushnumber(threadL, currentValue); // エイリアス 'v'
    lua_setglobal(threadL, "v");

    int ref = LUA_REFNIL;
    auto it = t_lua.compiledRegistry.find(expression);
    if (it == t_lua.compiledRegistry.end()) {
        std::string code = "return " + expression;
        if (luaL_loadstring(threadL, code.c_str()) != LUA_OK) {
            qWarning() << "[LuaHost] Parse error:" << QString::fromStdString(expression) << "->" << lua_tostring(threadL, -1);
            lua_pop(threadL, 1);
            return currentValue;
        }
        ref = luaL_ref(threadL, LUA_REGISTRYINDEX);
        t_lua.compiledRegistry[expression] = ref;
    } else {
        ref = it->second;
    }

    lua_rawgeti(threadL, LUA_REGISTRYINDEX, ref);
    int ret = lua_pcall(threadL, 0, 1, 0);

    if (ret != LUA_OK) {
        const char *errMsg = lua_tostring(threadL, -1);
        qWarning() << "[LuaHost] Expression eval error:" << QString::fromStdString(expression) << "->" << errMsg;
        lua_pop(threadL, 1);
        return currentValue;
    }

    if (lua_isnumber(threadL, -1) == 0) {
        lua_pop(threadL, 1);
        return currentValue;
    }

    double result = lua_tonumber(threadL, -1);
    lua_pop(threadL, 1);
    return result;
}

} // namespace AviQtl::Scripting
