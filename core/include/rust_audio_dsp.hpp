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
using AudioMeter = AviQtlAudioMeter;

[[nodiscard]] inline AudioStatus resampleStereoLinear(std::span<const float> input,
                                                      std::span<float> output,
                                                      double sourceRate) {
    return static_cast<AudioStatus>(aviqtl_audio_resample_stereo_linear(
        input.data(), input.size(), output.data(), output.size(), sourceRate));
}

[[nodiscard]] inline AudioStatus mixStereo(std::span<const float> clip,
                                           std::span<float> master,
                                           AudioMixParameters parameters,
                                           AudioMeter &meter) {
    return static_cast<AudioStatus>(aviqtl_audio_mix_stereo(
        clip.data(), clip.size(), master.data(), master.size(), parameters, &meter));
}

} // namespace AviQtl::RustCore
