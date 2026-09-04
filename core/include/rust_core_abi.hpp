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
    AVIQTL_RUST_CORE_CAPABILITY_AUDIO_BATCH_MIX = 1ULL << 5,
    AVIQTL_RUST_CORE_CAPABILITY_TIMELINE_EDIT = 1ULL << 6,
    AVIQTL_RUST_CORE_CAPABILITY_TIMELINE_DOMAIN = 1ULL << 7,
    AVIQTL_RUST_CORE_CAPABILITY_KEYFRAME_DOCUMENT = 1ULL << 8,
    AVIQTL_RUST_CORE_CAPABILITY_CORE_POLICY = 1ULL << 9,
    AVIQTL_RUST_CORE_CAPABILITY_SETTINGS_DOCUMENT = 1ULL << 10,
    AVIQTL_RUST_CORE_CAPABILITY_PRESET_DOCUMENT = 1ULL << 11,
    AVIQTL_RUST_CORE_CAPABILITY_PACKAGE_DOCUMENT = 1ULL << 12,
    AVIQTL_RUST_CORE_CAPABILITY_EFFECT_DOCUMENT = 1ULL << 13,
    AVIQTL_RUST_CORE_CAPABILITY_SCRIPT_DOCUMENT = 1ULL << 14,
    AVIQTL_RUST_CORE_CAPABILITY_PLUGIN_DOCUMENT = 1ULL << 15,
    AVIQTL_RUST_CORE_CAPABILITY_TIMELINE_STATE = 1ULL << 16,
    AVIQTL_RUST_CORE_CAPABILITY_AUDIO_PLANNING = 1ULL << 17,
    AVIQTL_RUST_CORE_CAPABILITY_PERMISSION_STATE = 1ULL << 18,
};

enum AviQtlCoreStatus : std::uint32_t {
    AVIQTL_RUST_CORE_STATUS_OK = 0,
    AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT = 1,
    AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS = 2,
    AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL = 3,
    AVIQTL_RUST_CORE_STATUS_INVALID_JSON = 4,
    AVIQTL_RUST_CORE_STATUS_UNSUPPORTED_VERSION = 5,
    AVIQTL_RUST_CORE_STATUS_LOCKED_LAYER = 6,
    AVIQTL_RUST_CORE_STATUS_STATE_CONFLICT = 7,
};

struct AviQtlTimelineState;
struct AviQtlTimelineBakePlan;
struct AviQtlPermissionState;

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

struct AviQtlAudioBatchTrack {
    const float *samples;
    std::size_t samples_length;
    AviQtlAudioMixParameters parameters;
    std::int32_t clip_id;
    std::uint32_t mute;
    std::uint32_t solo;
    std::uint32_t reserved;
};
static_assert(offsetof(AviQtlAudioBatchTrack, samples) == 0);
#if INTPTR_MAX == INT64_MAX
static_assert(sizeof(AviQtlAudioBatchTrack) == 72);
static_assert(alignof(AviQtlAudioBatchTrack) == 8);
static_assert(offsetof(AviQtlAudioBatchTrack, parameters) == 16);
static_assert(offsetof(AviQtlAudioBatchTrack, clip_id) == 56);
static_assert(offsetof(AviQtlAudioBatchTrack, reserved) == 68);
#endif

struct AviQtlAudioBatchResult {
    std::int32_t clip_id;
    std::uint32_t mixed;
    AviQtlAudioMeter meter;
};
static_assert(sizeof(AviQtlAudioBatchResult) == 24);
static_assert(alignof(AviQtlAudioBatchResult) == 4);
static_assert(offsetof(AviQtlAudioBatchResult, clip_id) == 0);
static_assert(offsetof(AviQtlAudioBatchResult, meter) == 8);

struct AviQtlAudioPlaybackContext {
    std::int32_t current_frame;
    std::int32_t samples_per_frame;
    std::int32_t sample_rate;
    std::int32_t reserved;
    double fps;
    double mixer_playback_speed;
};
static_assert(sizeof(AviQtlAudioPlaybackContext) == 32);
static_assert(alignof(AviQtlAudioPlaybackContext) == 8);
static_assert(offsetof(AviQtlAudioPlaybackContext, fps) == 16);
static_assert(offsetof(AviQtlAudioPlaybackContext, mixer_playback_speed) == 24);

struct AviQtlAudioPlaybackInput {
    std::int32_t clip_id;
    std::int32_t start_frame;
    std::int32_t duration_frames;
    std::int32_t previous_frame;
    double source_start_time;
    double playback_speed;
    double direct_time;
    double previous_phase;
    float fade_in_seconds;
    float fade_out_seconds;
    float volume;
    float master_volume;
    float pan;
    std::uint32_t mute;
    std::uint32_t solo;
    std::uint32_t limiter;
    std::uint32_t direct_mode;
    std::uint32_t decoder_available;
    std::uint32_t has_previous_phase;
    std::uint32_t reserved;
};
static_assert(sizeof(AviQtlAudioPlaybackInput) == 96);
static_assert(alignof(AviQtlAudioPlaybackInput) == 8);
static_assert(offsetof(AviQtlAudioPlaybackInput, source_start_time) == 16);
static_assert(offsetof(AviQtlAudioPlaybackInput, fade_in_seconds) == 48);
static_assert(offsetof(AviQtlAudioPlaybackInput, mute) == 68);
static_assert(offsetof(AviQtlAudioPlaybackInput, reserved) == 92);

struct AviQtlAudioPlaybackPlan {
    std::int32_t clip_id;
    std::uint32_t action;
    double source_start_time;
    double source_rate;
    double next_phase;
    std::int32_t source_sample_count;
    std::uint32_t report_meter;
    std::uint32_t mute;
    std::uint32_t solo;
    AviQtlAudioMixParameters parameters;
};
static_assert(sizeof(AviQtlAudioPlaybackPlan) == 88);
static_assert(alignof(AviQtlAudioPlaybackPlan) == 8);
static_assert(offsetof(AviQtlAudioPlaybackPlan, source_start_time) == 8);
static_assert(offsetof(AviQtlAudioPlaybackPlan, source_sample_count) == 32);
static_assert(offsetof(AviQtlAudioPlaybackPlan, parameters) == 48);

struct AviQtlWaveformContext {
    std::int32_t pixel_width;
    std::int32_t display_duration_frames;
    double fps;
    std::uint32_t has_audio_effect;
    std::uint32_t direct_mode;
    std::uint32_t linked_video;
    std::uint32_t reserved;
};
static_assert(sizeof(AviQtlWaveformContext) == 32);
static_assert(alignof(AviQtlWaveformContext) == 8);
static_assert(offsetof(AviQtlWaveformContext, fps) == 8);
static_assert(offsetof(AviQtlWaveformContext, linked_video) == 24);

struct AviQtlWaveformSamplingPoint {
    std::int32_t relative_frame;
    std::int32_t next_relative_frame;
};
static_assert(sizeof(AviQtlWaveformSamplingPoint) == 8);
static_assert(alignof(AviQtlWaveformSamplingPoint) == 4);
static_assert(offsetof(AviQtlWaveformSamplingPoint, next_relative_frame) == 4);

struct AviQtlWaveformEvaluatedPoint {
    double direct_time;
    double next_direct_time;
    double start_time;
    double speed_percent;
    double volume;
    double master_volume;
    double pan;
    double fade_in_seconds;
    double fade_out_seconds;
    std::uint32_t mute;
    std::uint32_t reserved;
};
static_assert(sizeof(AviQtlWaveformEvaluatedPoint) == 80);
static_assert(alignof(AviQtlWaveformEvaluatedPoint) == 8);
static_assert(offsetof(AviQtlWaveformEvaluatedPoint, speed_percent) == 24);
static_assert(offsetof(AviQtlWaveformEvaluatedPoint, mute) == 72);

struct AviQtlWaveformPlan {
    double source_start_seconds;
    double source_duration_seconds;
    float display_gain;
    std::uint32_t reserved;
};
static_assert(sizeof(AviQtlWaveformPlan) == 24);
static_assert(alignof(AviQtlWaveformPlan) == 8);
static_assert(offsetof(AviQtlWaveformPlan, display_gain) == 16);

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

struct AviQtlEffectParamEntry {
    std::uint32_t clip_id;
    std::uint8_t effect_index;
    std::uint8_t param_type;
    std::uint8_t reserved[2];
    std::uint8_t param_name[20];
    float value[4];
};
static_assert(sizeof(AviQtlEffectParamEntry) == 44);
static_assert(alignof(AviQtlEffectParamEntry) == 4);
static_assert(offsetof(AviQtlEffectParamEntry, param_name) == 8);
static_assert(offsetof(AviQtlEffectParamEntry, value) == 28);

struct AviQtlSceneBakeCounts {
    std::size_t render_count;
    std::size_t audio_count;
    std::size_t param_count;
    std::uint64_t clips_visited;
    std::uint64_t selected_effect_count;
    std::uint64_t numeric_batch_calls;
    std::uint64_t numeric_track_count;
};
static_assert(sizeof(AviQtlSceneBakeCounts) == 56);
static_assert(alignof(AviQtlSceneBakeCounts) == 8);
static_assert(offsetof(AviQtlSceneBakeCounts, clips_visited) == 24);

struct AviQtlTimelineClipGeometry {
    std::int32_t clip_id;
    std::int32_t layer;
    std::int32_t start_frame;
    std::int32_t duration_frames;
};
static_assert(sizeof(AviQtlTimelineClipGeometry) == 16);
static_assert(alignof(AviQtlTimelineClipGeometry) == 4);
static_assert(offsetof(AviQtlTimelineClipGeometry, clip_id) == 0);
static_assert(offsetof(AviQtlTimelineClipGeometry, duration_frames) == 12);

struct AviQtlTimelineMoveInput {
    std::int32_t clip_id;
    std::int32_t old_layer;
    std::int32_t old_start_frame;
    std::int32_t duration_frames;
    std::int32_t target_layer;
    std::int32_t target_start_frame;
};
static_assert(sizeof(AviQtlTimelineMoveInput) == 24);
static_assert(alignof(AviQtlTimelineMoveInput) == 4);
static_assert(offsetof(AviQtlTimelineMoveInput, clip_id) == 0);
static_assert(offsetof(AviQtlTimelineMoveInput, target_layer) == 16);

struct AviQtlTimelinePosition {
    std::int32_t frame;
    std::int32_t layer;
};
static_assert(sizeof(AviQtlTimelinePosition) == 8);
static_assert(alignof(AviQtlTimelinePosition) == 4);

struct AviQtlSceneSettings {
    std::int32_t width;
    std::int32_t height;
    double fps;
    std::int32_t total_frames;
    std::uint32_t grid_mode;
    double grid_bpm;
    double grid_offset;
    std::int32_t grid_interval;
    std::int32_t grid_subdivision;
    std::uint32_t enable_snap;
    std::int32_t magnetic_snap_range;
};
static_assert(sizeof(AviQtlSceneSettings) == 56);
static_assert(alignof(AviQtlSceneSettings) == 8);
static_assert(offsetof(AviQtlSceneSettings, width) == 0);
static_assert(offsetof(AviQtlSceneSettings, fps) == 8);
static_assert(offsetof(AviQtlSceneSettings, grid_bpm) == 24);
static_assert(offsetof(AviQtlSceneSettings, magnetic_snap_range) == 52);

extern "C" {

std::uint32_t aviqtl_core_abi_version();
std::uint64_t aviqtl_core_capabilities();

std::int32_t aviqtl_easing_kind_from_name(const std::uint8_t *value, std::size_t valueLength);
std::size_t aviqtl_easing_count();
const std::uint8_t *aviqtl_easing_name(std::uint32_t kind, std::size_t *outputLength);
double aviqtl_easing_evaluate(std::uint32_t kind, double t, const double *points, std::size_t pointsLength, AviQtlEasingParameters parameters);

std::uint32_t aviqtl_audio_resample_stereo_linear(const float *input, std::size_t inputLength, float *output, std::size_t outputLength, double sourceRate);
std::uint32_t aviqtl_audio_mix_stereo_batch(const AviQtlAudioBatchTrack *tracks, std::size_t tracksLength, float *master, std::size_t masterLength, AviQtlAudioBatchResult *results, std::size_t resultsLength);
std::uint32_t aviqtl_audio_plan_playback_batch(const AviQtlAudioPlaybackContext *context, const AviQtlAudioPlaybackInput *inputs, std::size_t inputsLength, AviQtlAudioPlaybackPlan *output, std::size_t outputLength);
std::uint32_t aviqtl_audio_waveform_sampling_points(const AviQtlWaveformContext *context, AviQtlWaveformSamplingPoint *output, std::size_t outputLength);
std::uint32_t aviqtl_audio_plan_waveform(const AviQtlWaveformContext *context, const AviQtlWaveformEvaluatedPoint *evaluated, std::size_t evaluatedLength, AviQtlWaveformPlan *output, std::size_t outputLength);

std::uint32_t aviqtl_numeric_keyframe_batch_evaluate(const AviQtlNumericTrackView *tracks, std::size_t tracksLength, std::int32_t frame, double *output, std::size_t outputLength);
std::uint32_t aviqtl_numeric_keyframe_evaluate_typed(const AviQtlNumericTrackView *track, std::int32_t frame, std::uint32_t discrete, double *output);
std::uint32_t aviqtl_numeric_interpolation_from_name(const std::uint8_t *value, std::size_t valueLength);

std::uint32_t aviqtl_timeline_bake_render(const AviQtlRenderBakeInput *input, AviQtlRenderBakeOutput *output);
std::uint32_t aviqtl_timeline_bake_audio(const AviQtlAudioBakeInput *input, AviQtlAudioBakeOutput *output);
std::uint32_t aviqtl_timeline_bake_plan_create(const std::uint8_t *input, std::size_t inputLength, AviQtlTimelineBakePlan **outputHandle);
void aviqtl_timeline_bake_plan_destroy(AviQtlTimelineBakePlan *handle);
std::uint32_t aviqtl_timeline_bake_plan_reset(AviQtlTimelineBakePlan *handle, const std::uint8_t *input, std::size_t inputLength);
std::size_t aviqtl_timeline_bake_plan_effect_count(AviQtlTimelineBakePlan *handle);
std::uint32_t aviqtl_timeline_bake_plan_evaluate(AviQtlTimelineBakePlan *handle, std::int32_t currentFrame, std::uint32_t fullBake, std::int32_t prefetchFrames, AviQtlRenderBakeOutput *renderOutput, std::size_t renderCapacity, AviQtlAudioBakeOutput *audioOutput, std::size_t audioCapacity, AviQtlEffectParamEntry *paramOutput, std::size_t paramCapacity, AviQtlSceneBakeCounts *counts);

std::uint32_t aviqtl_timeline_find_vacant_frame(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, const std::int32_t *excludedIds, std::size_t excludedIdsLength, std::int32_t layer, std::int32_t startFrame, std::int32_t durationFrames,
                                                std::int32_t *outputFrame);
std::uint32_t aviqtl_timeline_plan_batch_move(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, const AviQtlTimelineMoveInput *moves, std::size_t movesLength, const std::int32_t *lockedLayers, std::size_t lockedLayersLength,
                                              AviQtlTimelineClipGeometry *output, std::size_t outputLength);
std::uint32_t aviqtl_timeline_plan_delta_move(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, const std::int32_t *movingIds, std::size_t movingIdsLength, const std::int32_t *lockedLayers, std::size_t lockedLayersLength,
                                              std::int32_t deltaLayer, std::int32_t deltaFrame, AviQtlTimelineClipGeometry *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_timeline_resolve_drag(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, const std::int32_t *movingIds, std::size_t movingIdsLength, const std::int32_t *lockedLayers, std::size_t lockedLayersLength,
                                           std::int32_t primaryClipId, std::int32_t targetLayer, std::int32_t proposedStartFrame, AviQtlTimelinePosition *output);
std::uint32_t aviqtl_timeline_plan_resize(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, std::int32_t deltaStartFrame, std::int32_t deltaDurationFrames, AviQtlTimelineClipGeometry *output, std::size_t outputLength);
std::uint32_t aviqtl_timeline_plan_insert_layers(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, std::int32_t targetLayer, std::int32_t count, std::uint32_t above, AviQtlTimelineClipGeometry *output, std::size_t outputCapacity,
                                                 std::size_t *outputLength);
std::uint32_t aviqtl_timeline_plan_shift_layers(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, std::int32_t startLayer, std::int32_t endLayer, std::int32_t delta, AviQtlTimelineClipGeometry *output, std::size_t outputCapacity,
                                                std::size_t *outputLength);
std::uint32_t aviqtl_timeline_clipboard_duration(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength, std::int32_t *outputDuration);
std::uint32_t aviqtl_timeline_plan_clipboard_placement(const AviQtlTimelineClipGeometry *existing, std::size_t existingLength, const AviQtlTimelineClipGeometry *clipboard, std::size_t clipboardLength, std::int32_t requestedFrame, std::int32_t layerOffset,
                                                       AviQtlTimelineClipGeometry *output, std::size_t outputLength, std::int32_t *outputFrame);
std::uint32_t aviqtl_timeline_split_clip(const AviQtlTimelineClipGeometry *clip, std::int32_t frame, AviQtlTimelineClipGeometry *first, AviQtlTimelineClipGeometry *second);

std::uint32_t aviqtl_timeline_state_create(const std::uint8_t *input, std::size_t inputLength, std::int32_t nextClipHint, std::int32_t nextSceneHint, AviQtlTimelineState **outputHandle);
void aviqtl_timeline_state_destroy(AviQtlTimelineState *handle);
std::uint32_t aviqtl_timeline_state_reset(AviQtlTimelineState *handle, const std::uint8_t *input, std::size_t inputLength, std::int32_t nextClipHint, std::int32_t nextSceneHint);
std::uint32_t aviqtl_timeline_state_snapshot_json(AviQtlTimelineState *handle, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_timeline_state_plan_json(AviQtlTimelineState *handle, const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_timeline_state_apply_patch_json(AviQtlTimelineState *handle, const std::uint8_t *input, std::size_t inputLength);
std::uint32_t aviqtl_timeline_state_reserve_clip_ids(AviQtlTimelineState *handle, std::size_t count, std::int32_t *output, std::size_t outputLength);
std::uint32_t aviqtl_timeline_state_reserve_scene_ids(AviQtlTimelineState *handle, std::size_t count, std::int32_t *output, std::size_t outputLength);
std::int32_t aviqtl_timeline_state_next_clip_id(AviQtlTimelineState *handle);
std::int32_t aviqtl_timeline_state_next_scene_id(AviQtlTimelineState *handle);
std::uint32_t aviqtl_timeline_state_set_next_clip_hint(AviQtlTimelineState *handle, std::int32_t nextHint);

std::uint32_t aviqtl_timeline_normalize_scene_settings(const AviQtlSceneSettings *input, AviQtlSceneSettings *output);
std::int32_t aviqtl_timeline_snap_frame(double frame, std::uint32_t ignoreSnap, const AviQtlSceneSettings *settings, double timelineScale);
std::int32_t aviqtl_timeline_duration(const AviQtlTimelineClipGeometry *clips, std::size_t clipsLength);
std::int32_t aviqtl_timeline_clamp_scene_duration(std::int32_t requestedDuration, std::int32_t sceneDuration, double speed, std::int32_t offset);
std::uint32_t aviqtl_selection_replace(const std::int32_t *ids, std::size_t idsLength, std::int32_t requestedPrimary, std::int32_t *output, std::size_t outputCapacity, std::size_t *outputLength, std::int32_t *outputPrimary);
std::uint32_t aviqtl_selection_toggle(const std::int32_t *currentIds, std::size_t currentIdsLength, std::int32_t currentPrimary, std::int32_t toggledId, std::int32_t *output, std::size_t outputCapacity, std::size_t *outputLength,
                                      std::int32_t *outputPrimary);
std::uint32_t aviqtl_timeline_normalize_removal_indices(std::size_t length, const std::int32_t *indices, std::size_t indicesLength, std::int32_t minimumIndex, std::int32_t *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_timeline_plan_index_move(std::size_t length, std::int32_t oldIndex, std::int32_t newIndex, std::int32_t minimumIndex, std::int32_t *redo, std::int32_t *undo);
std::uint32_t aviqtl_timeline_plan_multi_reorder(std::size_t length, const std::int32_t *indices, std::size_t indicesLength, std::int32_t targetIndex, std::int32_t minimumIndex, std::int32_t *redo, std::int32_t *undo, std::size_t *outputSelectedCount);

std::uint32_t aviqtl_keyframe_document_apply_json(const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_keyframe_evaluate_json(const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);

std::int32_t aviqtl_media_playback_mode(const std::uint8_t *value, std::size_t valueLength);
std::uint32_t aviqtl_media_is_video_file(const std::uint8_t *value, std::size_t valueLength);
std::uint32_t aviqtl_audio_parameter_affects_duration(const std::uint8_t *value, std::size_t valueLength);
std::uint32_t aviqtl_audio_parameter_affects_waveform(const std::uint8_t *value, std::size_t valueLength);
double aviqtl_media_resolve_audio_time(double relativeTime, std::uint32_t directMode, double directTime, double startTime, double speed);
double aviqtl_media_resolve_video_time(std::int32_t relativeFrame, double sourceFps, std::uint32_t directMode, double directFrame, double startFrame, double speed);
std::int32_t aviqtl_media_max_video_duration_frames(std::int32_t totalFrameCount, double sourceFps, double speed, double startFrame, std::int32_t projectFps);
std::int32_t aviqtl_media_clamp_video_duration_frames(std::int32_t requestedDuration, std::int32_t totalFrameCount, double sourceFps, std::uint32_t directMode, double startFrame, double speed, std::int32_t projectFps);
std::int32_t aviqtl_media_clamp_audio_duration_frames(std::int32_t requestedDuration, double totalSeconds, std::uint32_t directMode, double startTime, double speed, std::int32_t projectFps);
std::int32_t aviqtl_permission_from_name(const std::uint8_t *value, std::size_t valueLength);
std::int32_t aviqtl_permission_for_api(const std::uint8_t *value, std::size_t valueLength);
std::int32_t aviqtl_permission_count();
const std::uint8_t *aviqtl_permission_name(std::int32_t permission, std::size_t *outputLength);
std::uint32_t aviqtl_permission_state_create(const std::uint8_t *input, std::size_t inputLength, AviQtlPermissionState **outputHandle);
void aviqtl_permission_state_destroy(AviQtlPermissionState *handle);
std::uint32_t aviqtl_permission_state_reset(AviQtlPermissionState *handle, const std::uint8_t *input, std::size_t inputLength);
std::uint32_t aviqtl_permission_state_snapshot_json(const AviQtlPermissionState *handle, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_permission_state_has(const AviQtlPermissionState *handle, const std::uint8_t *plugin, std::size_t pluginLength, std::int32_t permission);
std::uint32_t aviqtl_permission_state_grant(AviQtlPermissionState *handle, const std::uint8_t *plugin, std::size_t pluginLength, std::int32_t permission);
std::uint32_t aviqtl_permission_state_revoke(AviQtlPermissionState *handle, const std::uint8_t *plugin, std::size_t pluginLength, std::int32_t permission, std::uint32_t *pluginExisted);
std::uint32_t aviqtl_permission_state_grant_all(AviQtlPermissionState *handle, const std::uint8_t *plugin, std::size_t pluginLength);
std::uint32_t aviqtl_permission_state_revoke_all(AviQtlPermissionState *handle, const std::uint8_t *plugin, std::size_t pluginLength);
std::uint64_t aviqtl_permission_state_mask(const AviQtlPermissionState *handle, const std::uint8_t *plugin, std::size_t pluginLength);
std::uint32_t aviqtl_permission_state_is_authorized(const AviQtlPermissionState *handle, const std::uint8_t *plugin, std::size_t pluginLength);

std::uint32_t aviqtl_package_id_is_valid(const std::uint8_t *value, std::size_t valueLength);
std::int32_t aviqtl_package_type(const std::uint8_t *value, std::size_t valueLength);
std::uint32_t aviqtl_package_archive_path_is_safe(const std::uint8_t *value, std::size_t valueLength);
std::uint32_t aviqtl_recovery_id_is_valid(const std::uint8_t *value, std::size_t valueLength);
std::uint32_t aviqtl_recovery_snapshot_name_is_valid(const std::uint8_t *id, std::size_t idLength, const std::uint8_t *fileName, std::size_t fileNameLength);

std::uint32_t aviqtl_settings_defaults_json(const std::uint8_t *platformDefaults, std::size_t platformDefaultsLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_settings_merge_json(const std::uint8_t *base, std::size_t baseLength, const std::uint8_t *loaded, std::size_t loadedLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength, std::uint32_t *migrated);
std::uint32_t aviqtl_settings_persistent_json(const std::uint8_t *settings, std::size_t settingsLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);

std::uint32_t aviqtl_preset_name_is_safe(const std::uint8_t *value, std::size_t valueLength);
std::uint32_t aviqtl_preset_build_json(const std::uint8_t *effectId, std::size_t effectIdLength, const std::uint8_t *name, std::size_t nameLength, std::uint32_t enabled, const std::uint8_t *params, std::size_t paramsLength, const std::uint8_t *keyframes,
                                       std::size_t keyframesLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);
std::uint32_t aviqtl_preset_normalize_json(const std::uint8_t *effectId, std::size_t effectIdLength, const std::uint8_t *name, std::size_t nameLength, const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity,
                                           std::size_t *outputLength);

std::uint32_t aviqtl_package_document_apply_json(const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);

std::uint32_t aviqtl_effect_metadata_normalize_json(const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);

std::uint32_t aviqtl_script_metadata_parse_json(const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);

std::uint32_t aviqtl_plugin_document_apply_json(const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);

std::int32_t aviqtl_project_current_version();
std::uint32_t aviqtl_project_normalize_json(const std::uint8_t *input, std::size_t inputLength, std::uint8_t *output, std::size_t outputCapacity, std::size_t *outputLength);
}
