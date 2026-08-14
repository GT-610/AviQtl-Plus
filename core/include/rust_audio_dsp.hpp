#pragma once

#include <cstddef>
#include <cstdint>
#include <span>

extern "C" {

struct AviQtlAudioMixParameters {
    double relative_time;
    double duration;
    float fade_in_seconds;
    float fade_out_seconds;
    float volume;
    float master_volume;
    float pan;
    std::uint32_t limiter;
};
static_assert(sizeof(AviQtlAudioMixParameters) == 40);
static_assert(offsetof(AviQtlAudioMixParameters, relative_time) == 0);
static_assert(offsetof(AviQtlAudioMixParameters, duration) == 8);
static_assert(offsetof(AviQtlAudioMixParameters, fade_in_seconds) == 16);
static_assert(offsetof(AviQtlAudioMixParameters, limiter) == 36);

struct AviQtlAudioMeter {
    float peak_left;
    float peak_right;
    float rms_left;
    float rms_right;
};
static_assert(sizeof(AviQtlAudioMeter) == sizeof(float) * 4);

std::uint32_t aviqtl_audio_resample_stereo_linear(const float *input, std::size_t inputLength,
                                                  float *output, std::size_t outputLength,
                                                  double sourceRate);
std::uint32_t aviqtl_audio_mix_stereo(const float *clip, std::size_t clipLength,
                                     float *master, std::size_t masterLength,
                                     AviQtlAudioMixParameters parameters,
                                     AviQtlAudioMeter *meter);

}

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
