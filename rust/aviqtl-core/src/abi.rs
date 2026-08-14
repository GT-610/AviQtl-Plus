use std::mem::{align_of, size_of};

pub const ABI_VERSION: u32 = 1;
pub const CAPABILITY_EASING: u64 = 1 << 0;
pub const CAPABILITY_AUDIO_DSP: u64 = 1 << 1;
pub const CAPABILITY_NUMERIC_KEYFRAME_BATCH: u64 = 1 << 2;
pub const CAPABILITY_TIMELINE_BAKE: u64 = 1 << 3;
pub const CAPABILITIES: u64 = CAPABILITY_EASING
    | CAPABILITY_AUDIO_DSP
    | CAPABILITY_NUMERIC_KEYFRAME_BATCH
    | CAPABILITY_TIMELINE_BAKE;

pub const STATUS_OK: u32 = 0;
pub const STATUS_INVALID_ARGUMENT: u32 = 1;
pub const STATUS_OVERLAPPING_BUFFERS: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlEasingParameters {
    pub amplitude: f64,
    pub period: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
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
