#pragma once

#include <QLoggingCategory>
#include <lua.hpp>
#include <string>

Q_DECLARE_LOGGING_CATEGORY(lcScripting)

namespace AviQtl::Scripting {

class LuaHost {
  public:
    static LuaHost &instance();
    ~LuaHost();

    static double evaluate(const std::string &expression, double time, int index, double currentValue);

    // Shared Lua state setup: safe libraries + dangerous function removal
    static void setupSafeLuaState(lua_State *L);

  private:
    LuaHost();

    // コピーを無効化
    LuaHost(const LuaHost &) = delete;
    LuaHost &operator=(const LuaHost &) = delete;
};

} // namespace AviQtl::Scripting