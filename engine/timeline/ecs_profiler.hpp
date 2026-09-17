#pragma once
// Define AVIQTL_PROFILE at compile time to enable counters.
#ifdef AVIQTL_PROFILE
#include <atomic>
#include <cstdint>

namespace AviQtl::Engine::Timeline {

struct ECSProfiler {
    std::atomic<uint64_t> commitCount{0};
    std::atomic<uint64_t> denseMapHit{0};
    std::atomic<uint64_t> denseMapMiss{0};
    std::atomic<uint64_t> syncAliveRemoved{0};
    std::atomic<uint64_t> dirtyBitSetCount{0};

    static ECSProfiler &instance() {
        static ECSProfiler prof;
        return prof;
    }

  private:
    ECSProfiler() = default;
};

} // namespace AviQtl::Engine::Timeline

#define ECS_PROF_INC(counter) AviQtl::Engine::Timeline::ECSProfiler::instance().counter.fetch_add(1, std::memory_order_relaxed)

#else
// リリースビルドでは全マクロがゼロコスト
#define ECS_PROF_INC(counter) ((void)0)
#endif
