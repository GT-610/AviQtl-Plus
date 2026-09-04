use std::mem::{align_of, size_of};

pub const ABI_VERSION: u32 = 1;
pub const CAPABILITY_EASING: u64 = 1 << 0;
pub const CAPABILITY_AUDIO_DSP: u64 = 1 << 1;
pub const CAPABILITY_NUMERIC_KEYFRAME_BATCH: u64 = 1 << 2;
pub const CAPABILITY_TIMELINE_BAKE: u64 = 1 << 3;
pub const CAPABILITY_PROJECT_DOCUMENT: u64 = 1 << 4;
pub const CAPABILITY_AUDIO_BATCH_MIX: u64 = 1 << 5;
pub const CAPABILITY_TIMELINE_EDIT: u64 = 1 << 6;
pub const CAPABILITY_TIMELINE_DOMAIN: u64 = 1 << 7;
pub const CAPABILITY_KEYFRAME_DOCUMENT: u64 = 1 << 8;
pub const CAPABILITY_CORE_POLICY: u64 = 1 << 9;
pub const CAPABILITY_SETTINGS_DOCUMENT: u64 = 1 << 10;
pub const CAPABILITY_PRESET_DOCUMENT: u64 = 1 << 11;
pub const CAPABILITY_PACKAGE_DOCUMENT: u64 = 1 << 12;
pub const CAPABILITY_EFFECT_DOCUMENT: u64 = 1 << 13;
pub const CAPABILITY_SCRIPT_DOCUMENT: u64 = 1 << 14;
pub const CAPABILITY_PLUGIN_DOCUMENT: u64 = 1 << 15;
pub const CAPABILITY_TIMELINE_STATE: u64 = 1 << 16;
pub const CAPABILITY_AUDIO_PLANNING: u64 = 1 << 17;
pub const CAPABILITY_PERMISSION_STATE: u64 = 1 << 18;
pub const CAPABILITY_SETTINGS_STATE: u64 = 1 << 19;
pub const CAPABILITY_PACKAGE_CATALOG_STATE: u64 = 1 << 20;
pub const CAPABILITY_RECOVERY_DOCUMENT: u64 = 1 << 21;
pub const CAPABILITY_EXPORT_PLANNING: u64 = 1 << 22;
pub const CAPABILITY_EFFECT_CATALOG_STATE: u64 = 1 << 23;
pub const CAPABILITY_SCRIPT_PLUGIN_CATALOG_STATE: u64 = 1 << 24;
pub const CAPABILITIES: u64 = CAPABILITY_EASING
    | CAPABILITY_AUDIO_DSP
    | CAPABILITY_NUMERIC_KEYFRAME_BATCH
    | CAPABILITY_TIMELINE_BAKE
    | CAPABILITY_PROJECT_DOCUMENT
    | CAPABILITY_AUDIO_BATCH_MIX
    | CAPABILITY_TIMELINE_EDIT
    | CAPABILITY_TIMELINE_DOMAIN
    | CAPABILITY_KEYFRAME_DOCUMENT
    | CAPABILITY_CORE_POLICY
    | CAPABILITY_SETTINGS_DOCUMENT
    | CAPABILITY_PRESET_DOCUMENT
    | CAPABILITY_PACKAGE_DOCUMENT
    | CAPABILITY_EFFECT_DOCUMENT
    | CAPABILITY_SCRIPT_DOCUMENT
    | CAPABILITY_PLUGIN_DOCUMENT
    | CAPABILITY_TIMELINE_STATE
    | CAPABILITY_AUDIO_PLANNING
    | CAPABILITY_PERMISSION_STATE
    | CAPABILITY_SETTINGS_STATE
    | CAPABILITY_PACKAGE_CATALOG_STATE
    | CAPABILITY_RECOVERY_DOCUMENT
    | CAPABILITY_EXPORT_PLANNING
    | CAPABILITY_EFFECT_CATALOG_STATE
    | CAPABILITY_SCRIPT_PLUGIN_CATALOG_STATE;

pub const STATUS_OK: u32 = 0;
pub const STATUS_INVALID_ARGUMENT: u32 = 1;
pub const STATUS_OVERLAPPING_BUFFERS: u32 = 2;
pub const STATUS_BUFFER_TOO_SMALL: u32 = 3;
pub const STATUS_INVALID_JSON: u32 = 4;
pub const STATUS_UNSUPPORTED_VERSION: u32 = 5;
pub const STATUS_LOCKED_LAYER: u32 = 6;
pub const STATUS_STATE_CONFLICT: u32 = 7;

/// Decodes a caller-provided UTF-8 byte range.
///
/// # Safety
///
/// `value` must be valid for `length` readable bytes and remain alive for the returned
/// reference's lifetime. A null pointer is permitted only when `length` is zero.
pub(crate) unsafe fn utf8<'a>(value: *const u8, length: usize) -> Option<&'a str> {
    if !slice_is_valid(value, length) {
        return None;
    }
    let bytes = if length == 0 {
        &[]
    } else {
        // SAFETY: The caller upholds the readable-range and lifetime contract.
        unsafe { std::slice::from_raw_parts(value, length) }
    };
    std::str::from_utf8(bytes).ok()
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlEasingParameters {
    pub amplitude: f64,
    pub period: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioMixParameters {
    pub relative_time: f64,
    pub duration: f64,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub volume: f32,
    pub master_volume: f32,
    pub pan: f32,
    pub limiter: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioMeter {
    pub peak_left: f32,
    pub peak_right: f32,
    pub rms_left: f32,
    pub rms_right: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlAudioBatchTrack {
    pub samples: *const f32,
    pub samples_length: usize,
    pub parameters: AviQtlAudioMixParameters,
    pub clip_id: i32,
    pub mute: u32,
    pub solo: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioBatchResult {
    pub clip_id: i32,
    pub mixed: u32,
    pub meter: AviQtlAudioMeter,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioPlaybackContext {
    pub current_frame: i32,
    pub samples_per_frame: i32,
    pub sample_rate: i32,
    pub reserved: i32,
    pub fps: f64,
    pub mixer_playback_speed: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioPlaybackInput {
    pub clip_id: i32,
    pub start_frame: i32,
    pub duration_frames: i32,
    pub previous_frame: i32,
    pub source_start_time: f64,
    pub playback_speed: f64,
    pub direct_time: f64,
    pub previous_phase: f64,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub volume: f32,
    pub master_volume: f32,
    pub pan: f32,
    pub mute: u32,
    pub solo: u32,
    pub limiter: u32,
    pub direct_mode: u32,
    pub decoder_available: u32,
    pub has_previous_phase: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioPlaybackPlan {
    pub clip_id: i32,
    pub action: u32,
    pub source_start_time: f64,
    pub source_rate: f64,
    pub next_phase: f64,
    pub source_sample_count: i32,
    pub report_meter: u32,
    pub mute: u32,
    pub solo: u32,
    pub parameters: AviQtlAudioMixParameters,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlWaveformContext {
    pub pixel_width: i32,
    pub display_duration_frames: i32,
    pub fps: f64,
    pub has_audio_effect: u32,
    pub direct_mode: u32,
    pub linked_video: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlWaveformSamplingPoint {
    pub relative_frame: i32,
    pub next_relative_frame: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlWaveformEvaluatedPoint {
    pub direct_time: f64,
    pub next_direct_time: f64,
    pub start_time: f64,
    pub speed_percent: f64,
    pub volume: f64,
    pub master_volume: f64,
    pub pan: f64,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
    pub mute: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlWaveformPlan {
    pub source_start_seconds: f64,
    pub source_duration_seconds: f64,
    pub display_gain: f32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlNumericKeyframe {
    pub frame: i32,
    pub interpolation: u32,
    pub step_frames: u32,
    pub custom_points_offset: u32,
    pub custom_points_length: u32,
    pub reserved: u32,
    pub value: f64,
    pub amplitude: f64,
    pub period: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlNumericTrackView {
    pub keyframes: *const AviQtlNumericKeyframe,
    pub keyframes_length: usize,
    pub custom_points: *const f64,
    pub custom_points_length: usize,
    pub fallback_value: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlRenderBakeInput {
    pub clip_id: i32,
    pub layer: i32,
    pub current_frame: i32,
    pub start_frame: i32,
    pub duration_frames: i32,
    pub clip_by_upper_object: u32,
    pub effect_count: u16,
    pub reserved: u16,
    pub effect_start_index: u32,
    pub has_transform: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_x: f32,
    pub rotation_y: f32,
    pub rotation_z: f32,
    pub scale: f32,
    pub opacity: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlRenderBakeOutput {
    pub clip_id: i32,
    pub layer: i32,
    pub time_position: f64,
    pub start_frame: i32,
    pub duration_frames: i32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_x: f32,
    pub rotation_y: f32,
    pub rotation_z: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub opacity: f32,
    pub clip_by_upper_object: u32,
    pub effect_count: u16,
    pub reserved: u16,
    pub effect_start_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlAudioBakeInput {
    pub clip_id: i32,
    pub start_frame: i32,
    pub duration_frames: i32,
    pub has_audio_effect: u32,
    pub fps: f64,
    pub source_start_time: f32,
    pub speed_percent: f32,
    pub direct_time: f32,
    pub volume: f32,
    pub master_volume: f32,
    pub pan: f32,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub direct_mode: u32,
    pub mute: u32,
    pub solo: u32,
    pub limiter: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioBakeOutput {
    pub clip_id: i32,
    pub start_frame: i32,
    pub duration_frames: i32,
    pub source_start_time: f32,
    pub playback_speed: f32,
    pub direct_time: f32,
    pub volume: f32,
    pub master_volume: f32,
    pub pan: f32,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub mute: u32,
    pub solo: u32,
    pub limiter: u32,
    pub direct_mode: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlEffectParamEntry {
    pub clip_id: u32,
    pub effect_index: u8,
    pub param_type: u8,
    pub reserved: [u8; 2],
    pub param_name: [u8; 20],
    pub value: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlSceneBakeCounts {
    pub render_count: usize,
    pub audio_count: usize,
    pub param_count: usize,
    pub clips_visited: u64,
    pub selected_effect_count: u64,
    pub numeric_batch_calls: u64,
    pub numeric_track_count: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlTimelineClipGeometry {
    pub clip_id: i32,
    pub layer: i32,
    pub start_frame: i32,
    pub duration_frames: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlTimelineMoveInput {
    pub clip_id: i32,
    pub old_layer: i32,
    pub old_start_frame: i32,
    pub duration_frames: i32,
    pub target_layer: i32,
    pub target_start_frame: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlTimelinePosition {
    pub frame: i32,
    pub layer: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct AviQtlSceneSettings {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub total_frames: i32,
    pub grid_mode: u32,
    pub grid_bpm: f64,
    pub grid_offset: f64,
    pub grid_interval: i32,
    pub grid_subdivision: i32,
    pub enable_snap: u32,
    pub magnetic_snap_range: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlExportVideoDefaults {
    pub width: i32,
    pub height: i32,
    pub fps_num: i32,
    pub fps_den: i32,
    pub bitrate: i64,
    pub crf: i32,
    pub gop_size: i32,
    pub audio_bitrate: i64,
    pub start_frame: i32,
    pub end_frame: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct AviQtlExportVideoRequest {
    pub width: i32,
    pub height: i32,
    pub fps_num: i32,
    pub fps_den: i32,
    pub start_frame: i32,
    pub end_frame: i32,
    pub timeline_duration: i32,
    pub output_path_present: u32,
    pub project_fps: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlExportVideoPlan {
    pub start_frame: i32,
    pub end_frame: i32,
    pub total_frames: i32,
    pub error: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlExportImageSequenceRequest {
    pub start_frame: i32,
    pub end_frame: i32,
    pub timeline_duration: i32,
    pub configured_padding: i32,
    pub output_path_present: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlExportImageSequencePlan {
    pub start_frame: i32,
    pub end_frame: i32,
    pub total_frames: i32,
    pub pad_digits: i32,
    pub image_format: u32,
    pub error: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlExportAudioFramePlan {
    pub cumulative_samples: i64,
    pub samples_for_frame: i32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AviQtlExportProgressPlan {
    pub progress: i32,
    pub current_frame: i32,
    pub total_frames: i32,
    pub eta_seconds: i32,
    pub should_emit: u32,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct AviQtlIdAllocation {
    pub allocated_id: i32,
    pub next_id: i32,
}

pub fn pointer_is_valid<T>(pointer: *const T, length: usize) -> bool {
    length == 0 || (!pointer.is_null() && (pointer as usize) & (align_of::<T>() - 1) == 0)
}

fn byte_range<T>(pointer: *const T, length: usize) -> Option<(usize, usize)> {
    let byte_length = length.checked_mul(size_of::<T>())?;
    if byte_length > isize::MAX as usize {
        return None;
    }
    let start = pointer as usize;
    Some((start, start.checked_add(byte_length)?))
}

pub fn slice_is_valid<T>(pointer: *const T, length: usize) -> bool {
    pointer_is_valid(pointer, length) && (length == 0 || byte_range(pointer, length).is_some())
}

pub fn ranges_overlap<T, U>(
    first: *const T,
    first_length: usize,
    second: *const U,
    second_length: usize,
) -> Option<bool> {
    if first_length == 0 || second_length == 0 {
        return Some(false);
    }
    let (first_start, first_end) = byte_range(first, first_length)?;
    let (second_start, second_end) = byte_range(second, second_length)?;
    Some(first_start < second_end && second_start < first_end)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_core_abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_core_capabilities() -> u64 {
    CAPABILITIES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn exposed_layouts_match_the_c_abi_contract() {
        assert_eq!(size_of::<AviQtlEasingParameters>(), 16);
        assert_eq!(align_of::<AviQtlEasingParameters>(), 8);
        assert_eq!(offset_of!(AviQtlEasingParameters, amplitude), 0);
        assert_eq!(offset_of!(AviQtlEasingParameters, period), 8);

        assert_eq!(size_of::<AviQtlAudioMixParameters>(), 40);
        assert_eq!(align_of::<AviQtlAudioMixParameters>(), 8);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, relative_time), 0);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, duration), 8);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, fade_in_seconds), 16);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, limiter), 36);

        assert_eq!(size_of::<AviQtlAudioMeter>(), 16);
        assert_eq!(align_of::<AviQtlAudioMeter>(), 4);
        assert_eq!(offset_of!(AviQtlAudioMeter, peak_left), 0);
        assert_eq!(offset_of!(AviQtlAudioMeter, rms_right), 12);

        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(size_of::<AviQtlAudioBatchTrack>(), 72);
            assert_eq!(align_of::<AviQtlAudioBatchTrack>(), 8);
            assert_eq!(offset_of!(AviQtlAudioBatchTrack, samples), 0);
            assert_eq!(offset_of!(AviQtlAudioBatchTrack, parameters), 16);
            assert_eq!(offset_of!(AviQtlAudioBatchTrack, clip_id), 56);
            assert_eq!(offset_of!(AviQtlAudioBatchTrack, reserved), 68);
        }

        assert_eq!(size_of::<AviQtlAudioBatchResult>(), 24);
        assert_eq!(align_of::<AviQtlAudioBatchResult>(), 4);
        assert_eq!(offset_of!(AviQtlAudioBatchResult, clip_id), 0);
        assert_eq!(offset_of!(AviQtlAudioBatchResult, meter), 8);

        assert_eq!(size_of::<AviQtlAudioPlaybackContext>(), 32);
        assert_eq!(align_of::<AviQtlAudioPlaybackContext>(), 8);
        assert_eq!(offset_of!(AviQtlAudioPlaybackContext, fps), 16);
        assert_eq!(
            offset_of!(AviQtlAudioPlaybackContext, mixer_playback_speed),
            24
        );

        assert_eq!(size_of::<AviQtlAudioPlaybackInput>(), 96);
        assert_eq!(align_of::<AviQtlAudioPlaybackInput>(), 8);
        assert_eq!(offset_of!(AviQtlAudioPlaybackInput, source_start_time), 16);
        assert_eq!(offset_of!(AviQtlAudioPlaybackInput, fade_in_seconds), 48);
        assert_eq!(offset_of!(AviQtlAudioPlaybackInput, mute), 68);
        assert_eq!(offset_of!(AviQtlAudioPlaybackInput, reserved), 92);

        assert_eq!(size_of::<AviQtlAudioPlaybackPlan>(), 88);
        assert_eq!(align_of::<AviQtlAudioPlaybackPlan>(), 8);
        assert_eq!(offset_of!(AviQtlAudioPlaybackPlan, source_start_time), 8);
        assert_eq!(offset_of!(AviQtlAudioPlaybackPlan, source_sample_count), 32);
        assert_eq!(offset_of!(AviQtlAudioPlaybackPlan, parameters), 48);

        assert_eq!(size_of::<AviQtlWaveformContext>(), 32);
        assert_eq!(align_of::<AviQtlWaveformContext>(), 8);
        assert_eq!(offset_of!(AviQtlWaveformContext, fps), 8);
        assert_eq!(offset_of!(AviQtlWaveformContext, linked_video), 24);

        assert_eq!(size_of::<AviQtlWaveformSamplingPoint>(), 8);
        assert_eq!(align_of::<AviQtlWaveformSamplingPoint>(), 4);
        assert_eq!(
            offset_of!(AviQtlWaveformSamplingPoint, next_relative_frame),
            4
        );

        assert_eq!(size_of::<AviQtlWaveformEvaluatedPoint>(), 80);
        assert_eq!(align_of::<AviQtlWaveformEvaluatedPoint>(), 8);
        assert_eq!(offset_of!(AviQtlWaveformEvaluatedPoint, speed_percent), 24);
        assert_eq!(offset_of!(AviQtlWaveformEvaluatedPoint, mute), 72);

        assert_eq!(size_of::<AviQtlWaveformPlan>(), 24);
        assert_eq!(align_of::<AviQtlWaveformPlan>(), 8);
        assert_eq!(offset_of!(AviQtlWaveformPlan, display_gain), 16);

        assert_eq!(size_of::<AviQtlNumericKeyframe>(), 48);
        assert_eq!(align_of::<AviQtlNumericKeyframe>(), 8);
        assert_eq!(offset_of!(AviQtlNumericKeyframe, frame), 0);
        assert_eq!(offset_of!(AviQtlNumericKeyframe, interpolation), 4);
        assert_eq!(offset_of!(AviQtlNumericKeyframe, custom_points_length), 16);
        assert_eq!(offset_of!(AviQtlNumericKeyframe, value), 24);
        assert_eq!(offset_of!(AviQtlNumericKeyframe, period), 40);

        assert_eq!(size_of::<AviQtlRenderBakeInput>(), 68);
        assert_eq!(align_of::<AviQtlRenderBakeInput>(), 4);
        assert_eq!(offset_of!(AviQtlRenderBakeInput, clip_id), 0);
        assert_eq!(offset_of!(AviQtlRenderBakeInput, effect_count), 24);
        assert_eq!(offset_of!(AviQtlRenderBakeInput, effect_start_index), 28);
        assert_eq!(offset_of!(AviQtlRenderBakeInput, x), 36);
        assert_eq!(offset_of!(AviQtlRenderBakeInput, opacity), 64);
        assert_eq!(size_of::<AviQtlEffectParamEntry>(), 44);
        assert_eq!(align_of::<AviQtlEffectParamEntry>(), 4);
        assert_eq!(offset_of!(AviQtlEffectParamEntry, param_name), 8);
        assert_eq!(offset_of!(AviQtlEffectParamEntry, value), 28);
        assert_eq!(size_of::<AviQtlSceneBakeCounts>(), 56);
        assert_eq!(align_of::<AviQtlSceneBakeCounts>(), 8);
        assert_eq!(offset_of!(AviQtlSceneBakeCounts, clips_visited), 24);

        assert_eq!(size_of::<AviQtlRenderBakeOutput>(), 72);
        assert_eq!(align_of::<AviQtlRenderBakeOutput>(), 8);
        assert_eq!(offset_of!(AviQtlRenderBakeOutput, time_position), 8);
        assert_eq!(offset_of!(AviQtlRenderBakeOutput, x), 24);
        assert_eq!(offset_of!(AviQtlRenderBakeOutput, clip_by_upper_object), 60);
        assert_eq!(offset_of!(AviQtlRenderBakeOutput, effect_start_index), 68);

        assert_eq!(size_of::<AviQtlAudioBakeInput>(), 72);
        assert_eq!(align_of::<AviQtlAudioBakeInput>(), 8);
        assert_eq!(offset_of!(AviQtlAudioBakeInput, fps), 16);
        assert_eq!(offset_of!(AviQtlAudioBakeInput, source_start_time), 24);
        assert_eq!(offset_of!(AviQtlAudioBakeInput, direct_mode), 56);
        assert_eq!(offset_of!(AviQtlAudioBakeInput, limiter), 68);

        assert_eq!(size_of::<AviQtlAudioBakeOutput>(), 60);
        assert_eq!(align_of::<AviQtlAudioBakeOutput>(), 4);
        assert_eq!(offset_of!(AviQtlAudioBakeOutput, source_start_time), 12);
        assert_eq!(offset_of!(AviQtlAudioBakeOutput, mute), 44);
        assert_eq!(offset_of!(AviQtlAudioBakeOutput, direct_mode), 56);

        assert_eq!(size_of::<AviQtlTimelineClipGeometry>(), 16);
        assert_eq!(align_of::<AviQtlTimelineClipGeometry>(), 4);
        assert_eq!(offset_of!(AviQtlTimelineClipGeometry, clip_id), 0);
        assert_eq!(offset_of!(AviQtlTimelineClipGeometry, duration_frames), 12);

        assert_eq!(size_of::<AviQtlTimelineMoveInput>(), 24);
        assert_eq!(align_of::<AviQtlTimelineMoveInput>(), 4);
        assert_eq!(offset_of!(AviQtlTimelineMoveInput, clip_id), 0);
        assert_eq!(offset_of!(AviQtlTimelineMoveInput, target_layer), 16);

        assert_eq!(size_of::<AviQtlTimelinePosition>(), 8);
        assert_eq!(align_of::<AviQtlTimelinePosition>(), 4);

        assert_eq!(size_of::<AviQtlSceneSettings>(), 56);
        assert_eq!(align_of::<AviQtlSceneSettings>(), 8);
        assert_eq!(offset_of!(AviQtlSceneSettings, width), 0);
        assert_eq!(offset_of!(AviQtlSceneSettings, fps), 8);
        assert_eq!(offset_of!(AviQtlSceneSettings, grid_bpm), 24);
        assert_eq!(offset_of!(AviQtlSceneSettings, magnetic_snap_range), 52);

        assert_eq!(size_of::<AviQtlExportVideoDefaults>(), 48);
        assert_eq!(align_of::<AviQtlExportVideoDefaults>(), 8);
        assert_eq!(offset_of!(AviQtlExportVideoDefaults, bitrate), 16);
        assert_eq!(offset_of!(AviQtlExportVideoDefaults, audio_bitrate), 32);

        assert_eq!(size_of::<AviQtlExportVideoRequest>(), 40);
        assert_eq!(align_of::<AviQtlExportVideoRequest>(), 8);
        assert_eq!(offset_of!(AviQtlExportVideoRequest, project_fps), 32);

        assert_eq!(size_of::<AviQtlExportVideoPlan>(), 16);
        assert_eq!(align_of::<AviQtlExportVideoPlan>(), 4);
        assert_eq!(size_of::<AviQtlExportImageSequenceRequest>(), 20);
        assert_eq!(align_of::<AviQtlExportImageSequenceRequest>(), 4);
        assert_eq!(size_of::<AviQtlExportImageSequencePlan>(), 24);
        assert_eq!(align_of::<AviQtlExportImageSequencePlan>(), 4);
        assert_eq!(size_of::<AviQtlExportAudioFramePlan>(), 16);
        assert_eq!(align_of::<AviQtlExportAudioFramePlan>(), 8);
        assert_eq!(offset_of!(AviQtlExportAudioFramePlan, samples_for_frame), 8);
        assert_eq!(size_of::<AviQtlExportProgressPlan>(), 20);
        assert_eq!(align_of::<AviQtlExportProgressPlan>(), 4);

        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(size_of::<AviQtlNumericTrackView>(), 40);
            assert_eq!(align_of::<AviQtlNumericTrackView>(), 8);
            assert_eq!(offset_of!(AviQtlNumericTrackView, keyframes), 0);
            assert_eq!(offset_of!(AviQtlNumericTrackView, keyframes_length), 8);
            assert_eq!(offset_of!(AviQtlNumericTrackView, custom_points), 16);
            assert_eq!(offset_of!(AviQtlNumericTrackView, custom_points_length), 24);
            assert_eq!(offset_of!(AviQtlNumericTrackView, fallback_value), 32);
        }
    }

    #[test]
    fn reports_the_declared_version_and_capabilities() {
        assert_eq!(aviqtl_core_abi_version(), ABI_VERSION);
        assert_eq!(aviqtl_core_capabilities(), CAPABILITIES);
    }
}
