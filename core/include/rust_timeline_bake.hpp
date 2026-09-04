#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <limits>
#include <utility>
#include <vector>

namespace AviQtl::RustCore {

enum class TimelineBakeStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

using RenderBakeInput = AviQtlRenderBakeInput;
using RenderBakeOutput = AviQtlRenderBakeOutput;
using AudioBakeInput = AviQtlAudioBakeInput;
using AudioBakeOutput = AviQtlAudioBakeOutput;
using EffectParamEntry = AviQtlEffectParamEntry;
using SceneBakeCounts = AviQtlSceneBakeCounts;

struct SceneBakeResult {
    std::vector<RenderBakeOutput> renders;
    std::vector<AudioBakeOutput> audio;
    std::vector<EffectParamEntry> params;
    SceneBakeCounts counts{};
};

class TimelineBakePlan {
  public:
    TimelineBakePlan() = default;
    ~TimelineBakePlan() { aviqtl_timeline_bake_plan_destroy(m_handle); }

    TimelineBakePlan(const TimelineBakePlan &) = delete;
    TimelineBakePlan &operator=(const TimelineBakePlan &) = delete;

    TimelineBakePlan(TimelineBakePlan &&other) noexcept
        : m_handle(std::exchange(other.m_handle, nullptr)) {}

    TimelineBakePlan &operator=(TimelineBakePlan &&other) noexcept {
        if (this != &other) {
            aviqtl_timeline_bake_plan_destroy(m_handle);
            m_handle = std::exchange(other.m_handle, nullptr);
        }
        return *this;
    }

    [[nodiscard]] TimelineBakeStatus reset(const QByteArray &snapshot) {
        const auto *data = reinterpret_cast<const std::uint8_t *>(snapshot.constData());
        const auto size = static_cast<std::size_t>(snapshot.size());
        if (m_handle) {
            return static_cast<TimelineBakeStatus>(
                aviqtl_timeline_bake_plan_reset(m_handle, data, size));
        }
        AviQtlTimelineBakePlan *created = nullptr;
        const auto status = static_cast<TimelineBakeStatus>(
            aviqtl_timeline_bake_plan_create(data, size, &created));
        if (status == TimelineBakeStatus::Ok)
            m_handle = created;
        return status;
    }

    [[nodiscard]] std::size_t effectCount() const {
        return m_handle ? aviqtl_timeline_bake_plan_effect_count(m_handle) : 0;
    }

    [[nodiscard]] TimelineBakeStatus evaluate(int currentFrame, bool fullBake,
                                              int prefetchFrames, SceneBakeResult &result) {
        if (!m_handle)
            return TimelineBakeStatus::InvalidArgument;

        SceneBakeCounts required{};
        auto status = static_cast<TimelineBakeStatus>(aviqtl_timeline_bake_plan_evaluate(
            m_handle, currentFrame, static_cast<std::uint32_t>(fullBake), prefetchFrames,
            nullptr, 0, nullptr, 0, nullptr, 0, &required));
        if (status != TimelineBakeStatus::BufferTooSmall && status != TimelineBakeStatus::Ok)
            return status;
        if (required.render_count > result.renders.max_size() ||
            required.audio_count > result.audio.max_size() ||
            required.param_count > result.params.max_size()) {
            return TimelineBakeStatus::InvalidArgument;
        }

        result.renders.resize(required.render_count);
        result.audio.resize(required.audio_count);
        result.params.resize(required.param_count);
        result.counts = required;
        if (status == TimelineBakeStatus::Ok)
            return status;

        SceneBakeCounts written{};
        status = static_cast<TimelineBakeStatus>(aviqtl_timeline_bake_plan_evaluate(
            m_handle, currentFrame, static_cast<std::uint32_t>(fullBake), prefetchFrames,
            result.renders.data(), result.renders.size(), result.audio.data(),
            result.audio.size(), result.params.data(), result.params.size(), &written));
        if (status != TimelineBakeStatus::Ok)
            return status;
        if (written.render_count != result.renders.size() ||
            written.audio_count != result.audio.size() ||
            written.param_count != result.params.size()) {
            return TimelineBakeStatus::InvalidArgument;
        }
        result.counts = written;
        return TimelineBakeStatus::Ok;
    }

  private:
    AviQtlTimelineBakePlan *m_handle = nullptr;
};

[[nodiscard]] inline TimelineBakeStatus bakeRender(const RenderBakeInput &input,
                                                   RenderBakeOutput &output) {
    return static_cast<TimelineBakeStatus>(aviqtl_timeline_bake_render(&input, &output));
}

[[nodiscard]] inline TimelineBakeStatus bakeAudio(const AudioBakeInput &input,
                                                  AudioBakeOutput &output) {
    return static_cast<TimelineBakeStatus>(aviqtl_timeline_bake_audio(&input, &output));
}

} // namespace AviQtl::RustCore
