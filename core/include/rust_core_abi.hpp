#pragma once

#include <cstddef>
#include <cstdint>

inline constexpr std::uint32_t AVIQTL_RUST_CORE_ABI_VERSION = 1;

enum AviQtlCoreCapability : std::uint64_t {
    AVIQTL_RUST_CORE_CAPABILITY_EASING = 1ULL << 0,
    AVIQTL_RUST_CORE_CAPABILITY_AUDIO_DSP = 1ULL << 1,
    AVIQTL_RUST_CORE_CAPABILITY_NUMERIC_KEYFRAME_BATCH = 1ULL << 2,
    AVIQTL_RUST_CORE_CAPABILITY_TIMELINE_BAKE = 1ULL << 3,
    AVIQTL_RUST_CORE_CAPABILITY_PROJECT_DOCUMENT = 1ULL << 4,
};

enum AviQtlCoreStatus : std::uint32_t {
    AVIQTL_RUST_CORE_STATUS_OK = 0,
    AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT = 1,
    AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS = 2,
    AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL = 3,
    AVIQTL_RUST_CORE_STATUS_INVALID_JSON = 4,
    AVIQTL_RUST_CORE_STATUS_UNSUPPORTED_VERSION = 5,
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

struct AviQtlNumericKeyframe {
    std::int32_t frame;
    std::uint32_t interpolation;
    std::uint32_t step_frames;
    std::uint32_t custom_points_offset;
    std::uint32_t custom_points_length;
    std::uint32_t reserved;
    double value;
    double amplitude;
    double period;
};
static_assert(sizeof(AviQtlNumericKeyframe) == 48);
static_assert(alignof(AviQtlNumericKeyframe) == 8);
static_assert(offsetof(AviQtlNumericKeyframe, frame) == 0);
static_assert(offsetof(AviQtlNumericKeyframe, interpolation) == 4);
static_assert(offsetof(AviQtlNumericKeyframe, custom_points_length) == 16);
static_assert(offsetof(AviQtlNumericKeyframe, value) == 24);
static_assert(offsetof(AviQtlNumericKeyframe, period) == 40);

struct AviQtlNumericTrackView {
    const AviQtlNumericKeyframe *keyframes;
    std::size_t keyframes_length;
    const double *custom_points;
    std::size_t custom_points_length;
    double fallback_value;
};
static_assert(offsetof(AviQtlNumericTrackView, keyframes) == 0);
static_assert(offsetof(AviQtlNumericTrackView, fallback_value) == sizeof(void *) * 2 + sizeof(std::size_t) * 2);
#if INTPTR_MAX == INT64_MAX
static_assert(sizeof(AviQtlNumericTrackView) == 40);
static_assert(alignof(AviQtlNumericTrackView) == 8);
#endif

struct AviQtlRenderBakeInput {
    std::int32_t clip_id;
    std::int32_t layer;
    std::int32_t current_frame;
    std::int32_t start_frame;
    std::int32_t duration_frames;
    std::uint32_t clip_by_upper_object;
    std::uint16_t effect_count;
    std::uint16_t reserved;
    std::uint32_t effect_start_index;
    std::uint32_t has_transform;
    float x;
    float y;
    float z;
    float rotation_x;
    float rotation_y;
    float rotation_z;
    float scale;
    float opacity;
};
static_assert(sizeof(AviQtlRenderBakeInput) == 68);
static_assert(alignof(AviQtlRenderBakeInput) == 4);
static_assert(offsetof(AviQtlRenderBakeInput, clip_id) == 0);
static_assert(offsetof(AviQtlRenderBakeInput, effect_count) == 24);
static_assert(offsetof(AviQtlRenderBakeInput, effect_start_index) == 28);
static_assert(offsetof(AviQtlRenderBakeInput, x) == 36);
static_assert(offsetof(AviQtlRenderBakeInput, opacity) == 64);

struct AviQtlRenderBakeOutput {
    std::int32_t clip_id;
    std::int32_t layer;
    double time_position;
    std::int32_t start_frame;
    std::int32_t duration_frames;
    float x;
    float y;
    float z;
    float rotation_x;
    float rotation_y;
    float rotation_z;
    float scale_x;
    float scale_y;
    float opacity;
    std::uint32_t clip_by_upper_object;
    std::uint16_t effect_count;
    std::uint16_t reserved;
    std::uint32_t effect_start_index;
};
static_assert(sizeof(AviQtlRenderBakeOutput) == 72);
static_assert(alignof(AviQtlRenderBakeOutput) == 8);
static_assert(offsetof(AviQtlRenderBakeOutput, time_position) == 8);
static_assert(offsetof(AviQtlRenderBakeOutput, x) == 24);
static_assert(offsetof(AviQtlRenderBakeOutput, clip_by_upper_object) == 60);
static_assert(offsetof(AviQtlRenderBakeOutput, effect_start_index) == 68);

struct AviQtlAudioBakeInput {
    std::int32_t clip_id;
    std::int32_t start_frame;
    std::int32_t duration_frames;
    std::uint32_t has_audio_effect;
    double fps;
    float source_start_time;
    float speed_percent;
    float direct_time;
    float volume;
    float master_volume;
    float pan;
    float fade_in_seconds;
    float fade_out_seconds;
    std::uint32_t direct_mode;
    std::uint32_t mute;
    std::uint32_t solo;
    std::uint32_t limiter;
};
static_assert(sizeof(AviQtlAudioBakeInput) == 72);
static_assert(alignof(AviQtlAudioBakeInput) == 8);
static_assert(offsetof(AviQtlAudioBakeInput, fps) == 16);
static_assert(offsetof(AviQtlAudioBakeInput, source_start_time) == 24);
static_assert(offsetof(AviQtlAudioBakeInput, direct_mode) == 56);
static_assert(offsetof(AviQtlAudioBakeInput, limiter) == 68);

struct AviQtlAudioBakeOutput {
    std::int32_t clip_id;
    std::int32_t start_frame;
    std::int32_t duration_frames;
    float source_start_time;
    float playback_speed;
    float direct_time;
    float volume;
    float master_volume;
    float pan;
    float fade_in_seconds;
    float fade_out_seconds;
    std::uint32_t mute;
    std::uint32_t solo;
    std::uint32_t limiter;
    std::uint32_t direct_mode;
};
static_assert(sizeof(AviQtlAudioBakeOutput) == 60);
static_assert(alignof(AviQtlAudioBakeOutput) == 4);
static_assert(offsetof(AviQtlAudioBakeOutput, source_start_time) == 12);
static_assert(offsetof(AviQtlAudioBakeOutput, mute) == 44);
static_assert(offsetof(AviQtlAudioBakeOutput, direct_mode) == 56);

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

std::uint32_t aviqtl_numeric_keyframe_batch_evaluate(const AviQtlNumericTrackView *tracks,
                                                     std::size_t tracksLength,
                                                     std::int32_t frame,
                                                     double *output,
                                                     std::size_t outputLength);

std::uint32_t aviqtl_timeline_bake_render(const AviQtlRenderBakeInput *input,
                                          AviQtlRenderBakeOutput *output);
std::uint32_t aviqtl_timeline_bake_audio(const AviQtlAudioBakeInput *input,
                                         AviQtlAudioBakeOutput *output);

std::uint32_t aviqtl_project_normalize_json(const std::uint8_t *input,
                                            std::size_t inputLength,
                                            std::uint8_t *output,
                                            std::size_t outputCapacity,
                                            std::size_t *outputLength);

}
