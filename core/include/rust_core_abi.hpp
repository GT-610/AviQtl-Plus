#pragma once

#include <cstddef>
#include <cstdint>

inline constexpr std::uint32_t AVIQTL_RUST_CORE_ABI_VERSION = 1;

enum AviQtlCoreCapability : std::uint64_t {
    AVIQTL_RUST_CORE_CAPABILITY_EASING = 1ULL << 0,
    AVIQTL_RUST_CORE_CAPABILITY_AUDIO_DSP = 1ULL << 1,
};

enum AviQtlCoreStatus : std::uint32_t {
    AVIQTL_RUST_CORE_STATUS_OK = 0,
    AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT = 1,
    AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS = 2,
};

struct AviQtlEasingParameters {
    double amplitude;
    double period;
};
static_assert(sizeof(AviQtlEasingParameters) == 16);
static_assert(alignof(AviQtlEasingParameters) == 8);
static_assert(offsetof(AviQtlEasingParameters, amplitude) == 0);
static_assert(offsetof(AviQtlEasingParameters, period) == 8);

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
static_assert(alignof(AviQtlAudioMixParameters) == 8);
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
static_assert(sizeof(AviQtlAudioMeter) == 16);
static_assert(alignof(AviQtlAudioMeter) == 4);
static_assert(offsetof(AviQtlAudioMeter, peak_left) == 0);
static_assert(offsetof(AviQtlAudioMeter, rms_right) == 12);

extern "C" {

std::uint32_t aviqtl_core_abi_version();
std::uint64_t aviqtl_core_capabilities();

double aviqtl_solve_bezier_t(double x, double x1, double x2);
double aviqtl_easing_evaluate(std::uint32_t kind, double t, const double *points,
                              std::size_t pointsLength, AviQtlEasingParameters parameters);

std::uint32_t aviqtl_audio_resample_stereo_linear(const float *input, std::size_t inputLength,
                                                  float *output, std::size_t outputLength,
                                                  double sourceRate);
std::uint32_t aviqtl_audio_mix_stereo(const float *clip, std::size_t clipLength,
                                     float *master, std::size_t masterLength,
                                     AviQtlAudioMixParameters parameters,
                                     AviQtlAudioMeter *meter);

}
