#include "performance_metrics.hpp"

#include <QJsonValue>
#include <QVariant>
#include <array>

namespace AviQtl::Core {

namespace {

constexpr std::array<const char *, kPerformanceCounterCount> kCounterNames = {
    "bake_calls",
    "bake_nanoseconds",
    "bake_clips_visited",
    "bake_clips_active",
    "bake_track_cache_hits",
    "bake_track_cache_misses",
    "bake_rust_keyframe_batch_calls",
    "bake_rust_keyframe_tracks",
    "ecs_bridge_syncs",
    "ecs_bridge_nanoseconds",
    "ecs_bridge_states_reused",
    "ecs_bridge_states_rebuilt",
    "rhi_prepare_calls",
    "rhi_pipeline_rebuilds",
    "rhi_srb_rebuilds",
    "rhi_texture_rebuilds",
    "rhi_resource_creates",
    "rhi_resource_destroys",
    "decode_requests",
    "decode_requests_coalesced",
    "decode_cache_hits",
    "decode_frames_produced",
    "decode_hardware_downloads",
    "decode_pixel_conversions",
    "decode_nanoseconds",
    "export_frames",
    "export_frame_wait_nanoseconds",
    "export_frame_grab_nanoseconds",
    "export_encoder_queue_nanoseconds",
};

static_assert(kCounterNames.size() == kPerformanceCounterCount);

[[nodiscard]] constexpr std::size_t counterIndex(PerformanceCounter counter) {
    return static_cast<std::size_t>(counter);
}

} // namespace

quint64 PerformanceSnapshot::value(PerformanceCounter counter) const { return values[counterIndex(counter)]; }

QJsonObject PerformanceSnapshot::toJson() const {
    QJsonObject object;
    for (std::size_t i = 0; i < values.size(); ++i) {
        object.insert(QString::fromLatin1(kCounterNames[i]), QJsonValue::fromVariant(QVariant::fromValue(values[i])));
    }
    return object;
}

PerformanceMetrics &PerformanceMetrics::instance() {
    static PerformanceMetrics metrics;
    return metrics;
}

PerformanceMetrics::PerformanceMetrics() { reset(); }

void PerformanceMetrics::reset() {
    for (auto &value : m_values)
        value.store(0, std::memory_order_relaxed);
}

void PerformanceMetrics::add(PerformanceCounter counter, quint64 value) {
    m_values[counterIndex(counter)].fetch_add(value, std::memory_order_relaxed);
}

PerformanceSnapshot PerformanceMetrics::snapshot() const {
    PerformanceSnapshot result;
    for (std::size_t i = 0; i < m_values.size(); ++i)
        result.values[i] = m_values[i].load(std::memory_order_relaxed);
    return result;
}

QJsonObject PerformanceMetrics::report(const QString &scenario, const QJsonObject &context) const {
    QJsonObject result;
    result.insert(QStringLiteral("scenario"), scenario);
    result.insert(QStringLiteral("context"), context);
    result.insert(QStringLiteral("metrics"), snapshot().toJson());
    return result;
}

ScopedPerformanceTimer::ScopedPerformanceTimer(PerformanceCounter counter) : m_counter(counter), m_start(std::chrono::steady_clock::now()) {}

ScopedPerformanceTimer::~ScopedPerformanceTimer() {
    const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now() - m_start).count();
    PerformanceMetrics::instance().add(m_counter, static_cast<quint64>(elapsed));
}

} // namespace AviQtl::Core
