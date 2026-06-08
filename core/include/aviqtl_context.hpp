#pragma once
#include <memory>

namespace AviQtl::Core {
class Context {
  public:
    static Context &instance() {
        static Context inst;
        return inst;
    }
  private:
    Context() = default;
};
} // namespace AviQtl::Core
