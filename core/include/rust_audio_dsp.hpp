#pragma once

#include "rust_core_abi.hpp"
#include <cstdint>
#include <span>

namespace AviQtl::RustCore {

enum class AudioStatus : std::uint32_t {
    Ok = 0,
    InvalidArgument = 1,
    OverlappingBuffers = 2,
};

enum class AudioPlaybackAction : std::uint32_t {
    Skip = 0,
    Silence = 1,
    FetchDirect = 2,
    FetchResample = 3,
};

using AudioMixParameters = AviQtlAudioMixParameters;
using AudioBatchTrack = AviQtlAudioBatchTrack;
using AudioBatchResult = AviQtlAudioBatchResult;
using AudioPlaybackContext = AviQtlAudioPlaybackContext;
using AudioPlaybackInput = AviQtlAudioPlaybackInput;
using AudioPlaybackPlan = AviQtlAudioPlaybackPlan;
using WaveformContext = AviQtlWaveformContext;
using WaveformSamplingPoint = AviQtlWaveformSamplingPoint;
using WaveformEvaluatedPoint = AviQtlWaveformEvaluatedPoint;
using WaveformPlan = AviQtlWaveformPlan;

[[nodiscard]] inline AudioStatus resampleStereoLinear(std::span<const float> input,
                                                      std::span<float> output,
                                                      double sourceRate) {
    return static_cast<AudioStatus>(aviqtl_audio_resample_stereo_linear(
        input.data(), input.size(), output.data(), output.size(), sourceRate));
}

[[nodiscard]] inline AudioStatus mixStereoBatch(std::span<const AudioBatchTrack> tracks,
                                                std::span<float> master,
                                                std::span<AudioBatchResult> results) {
    return static_cast<AudioStatus>(aviqtl_audio_mix_stereo_batch(
        tracks.data(), tracks.size(), master.data(), master.size(), results.data(),
        results.size()));
}

[[nodiscard]] inline AudioStatus planPlaybackBatch(const AudioPlaybackContext &context,
                                                   std::span<const AudioPlaybackInput> inputs,
                                                   std::span<AudioPlaybackPlan> output) {
    return static_cast<AudioStatus>(aviqtl_audio_plan_playback_batch(
        &context, inputs.data(), inputs.size(), output.data(), output.size()));
}

[[nodiscard]] inline AudioStatus waveformSamplingPoints(const WaveformContext &context,
                                                        std::span<WaveformSamplingPoint> output) {
    return static_cast<AudioStatus>(
        aviqtl_audio_waveform_sampling_points(&context, output.data(), output.size()));
}

[[nodiscard]] inline AudioStatus planWaveform(const WaveformContext &context,
                                              std::span<const WaveformEvaluatedPoint> evaluated,
                                              std::span<WaveformPlan> output) {
    return static_cast<AudioStatus>(aviqtl_audio_plan_waveform(
        &context, evaluated.data(), evaluated.size(), output.data(), output.size()));
}

} // namespace AviQtl::RustCore
