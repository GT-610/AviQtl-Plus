#pragma once

#include <QJsonObject>
#include <QString>
#include <array>
#include <atomic>
#include <chrono>
#include <cstddef>

namespace AviQtl::Core {

enum class PerformanceCounter : std::size_t {
    BakeCalls,
    BakeNanoseconds,
    BakeClipsVisited,
    BakeClipsActive,
    BakeTrackCacheHits,
    BakeTrackCacheMisses,
    BakeRustKeyframeBatchCalls,
    BakeRustKeyframeTracks,
    EcsBridgeSyncs,
    EcsBridgeNanoseconds,
    EcsBridgeStatesReused,
    EcsBridgeStatesRebuilt,
    RhiPrepareCalls,
    RhiPipelineRebuilds,
    RhiSrbRebuilds,
    RhiTextureRebuilds,
    RhiResourceCreates,
    RhiResourceDestroys,
    DecodeRequests,
    DecodeRequestsCoalesced,
    DecodeCacheHits,
    DecodeFramesProduced,
    DecodeHardwareDownloads,
    DecodePixelConversions,
    DecodeNanoseconds,
    ExportFrames,
    ExportFrameWaitNanoseconds,
    ExportFrameGrabNanoseconds,
    ExportEncoderQueueNanoseconds,
    Count,
};

inline constexpr std::size_t kPerformanceCounterCount = static_cast<std::size_t>(PerformanceCounter::Count);

struct PerformanceSnapshot {
    std::array<quint64, kPerformanceCounterCount> values{};

    [[nodiscard]] quint64 value(PerformanceCounter counter) const;
    [[nodiscard]] QJsonObject toJson() const;
};

class PerformanceMetrics final {
  public:
    static PerformanceMetrics &instance();

    void reset();
    void add(PerformanceCounter counter, quint64 value = 1);
    [[nodiscard]] PerformanceSnapshot snapshot() const;
    [[nodiscard]] QJsonObject report(const QString &scenario, const QJsonObject &context = {}) const;

  private:
    PerformanceMetrics();

    std::array<std::atomic<quint64>, kPerformanceCounterCount> m_values;
};

class ScopedPerformanceTimer final {
  public:
    explicit ScopedPerformanceTimer(PerformanceCounter counter);
    ~ScopedPerformanceTimer();

    ScopedPerformanceTimer(const ScopedPerformanceTimer &) = delete;
    ScopedPerformanceTimer &operator=(const ScopedPerformanceTimer &) = delete;

  private:
    PerformanceCounter m_counter;
    std::chrono::steady_clock::time_point m_start;
};

} // namespace AviQtl::Core
