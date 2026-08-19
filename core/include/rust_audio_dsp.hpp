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

using AudioMixParameters = AviQtlAudioMixParameters;
using AudioBatchTrack = AviQtlAudioBatchTrack;
using AudioBatchResult = AviQtlAudioBatchResult;

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

} // namespace AviQtl::RustCore
