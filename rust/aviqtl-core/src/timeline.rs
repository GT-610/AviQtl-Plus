use crate::abi::{
    AviQtlAudioBakeInput, AviQtlAudioBakeOutput, AviQtlRenderBakeInput, AviQtlRenderBakeOutput,
    STATUS_INVALID_ARGUMENT, STATUS_OK, STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use std::cmp::Ordering;

const DEFAULT_SPEED_PERCENT: f32 = 100.0;

fn validate_io<T, U>(input: *const T, output: *mut U) -> u32 {
    if !slice_is_valid(input, 1) || !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(input, 1, output, 1) {
        Some(true) => STATUS_OVERLAPPING_BUFFERS,
        Some(false) => STATUS_OK,
        None => STATUS_INVALID_ARGUMENT,
    }
}

fn audio_defaults() -> AviQtlAudioBakeOutput {
    AviQtlAudioBakeOutput {
        clip_id: -1,
        playback_speed: 1.0,
        volume: 1.0,
        master_volume: 1.0,
        ..AviQtlAudioBakeOutput::default()
    }
}

/// Builds a render component from caller-owned plain data.
///
/// # Safety
///
/// `input` must point to one initialized input value and `output` to one writable output value.
/// Both pointers must be aligned and valid for the duration of the call, and their ranges must
/// not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_bake_render(
    input: *const AviQtlRenderBakeInput,
    output: *mut AviQtlRenderBakeOutput,
) -> u32 {
    let status = validate_io(input, output);
    if status != STATUS_OK {
        return status;
    }

    // SAFETY: Both single-value ranges were validated above and do not overlap.
    let input = unsafe { input.read() };
    let relative_frame = (i64::from(input.current_frame) - i64::from(input.start_frame)).max(0);
    let has_transform = input.has_transform != 0;
    let scale = if has_transform {
        input.scale * 0.01
    } else {
        1.0
    };
    let baked = AviQtlRenderBakeOutput {
        clip_id: input.clip_id,
        layer: input.layer,
        time_position: relative_frame as f64,
        start_frame: input.start_frame,
        duration_frames: input.duration_frames,
        x: if has_transform { input.x } else { 0.0 },
        y: if has_transform { input.y } else { 0.0 },
        z: if has_transform { input.z } else { 0.0 },
        rotation_x: if has_transform { input.rotation_x } else { 0.0 },
        rotation_y: if has_transform { input.rotation_y } else { 0.0 },
        rotation_z: if has_transform { input.rotation_z } else { 0.0 },
        scale_x: scale,
        scale_y: scale,
        opacity: if has_transform { input.opacity } else { 1.0 },
        clip_by_upper_object: u32::from(input.clip_by_upper_object != 0),
        effect_count: input.effect_count,
        reserved: 0,
        effect_start_index: input.effect_start_index,
    };
    // SAFETY: The output range was validated as aligned, writable, and non-overlapping.
    unsafe { output.write(baked) };
    STATUS_OK
}

/// Builds an audio component from caller-owned plain data.
///
/// # Safety
///
/// `input` must point to one initialized input value and `output` to one writable output value.
/// Both pointers must be aligned and valid for the duration of the call, and their ranges must
/// not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_bake_audio(
    input: *const AviQtlAudioBakeInput,
    output: *mut AviQtlAudioBakeOutput,
) -> u32 {
    let status = validate_io(input, output);
    if status != STATUS_OK {
        return status;
    }

    // SAFETY: Both single-value ranges were validated above and do not overlap.
    let input = unsafe { input.read() };
    let mut baked = audio_defaults();
    // Preserve the legacy C++ `fps <= 0.0` guard exactly: NaN and positive infinity
    // follow the active path.
    if matches!(input.fps.partial_cmp(&0.0), Some(Ordering::Greater) | None) {
        baked.clip_id = input.clip_id;
        baked.start_frame = input.start_frame;
        baked.duration_frames = input.duration_frames;
        if input.has_audio_effect != 0 {
            baked.source_start_time = input.source_start_time.max(0.0);
            baked.playback_speed = input.speed_percent.max(0.0) / DEFAULT_SPEED_PERCENT;
            baked.direct_time = input.direct_time.max(0.0);
            baked.volume = input.volume.max(0.0);
            baked.master_volume = input.master_volume.max(0.0);
            baked.pan = input.pan.clamp(-1.0, 1.0);
            baked.fade_in_seconds = input.fade_in_seconds.max(0.0);
            baked.fade_out_seconds = input.fade_out_seconds.max(0.0);
            baked.mute = u32::from(input.mute != 0);
            baked.solo = u32::from(input.solo != 0);
            baked.limiter = u32::from(input.limiter != 0);
            baked.direct_mode = u32::from(input.direct_mode != 0);
        }
    }
    // SAFETY: The output range was validated as aligned, writable, and non-overlapping.
    unsafe { output.write(baked) };
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{MaybeUninit, size_of};

    fn render_input() -> AviQtlRenderBakeInput {
        AviQtlRenderBakeInput {
            clip_id: 7,
            layer: 3,
            current_frame: 25,
            start_frame: 10,
            duration_frames: 100,
            clip_by_upper_object: 1,
            effect_count: 4,
            reserved: 0,
            effect_start_index: 9,
            has_transform: 1,
            x: 1.0,
            y: 2.0,
            z: 3.0,
            rotation_x: 4.0,
            rotation_y: 5.0,
            rotation_z: 6.0,
            scale: 125.0,
            opacity: 0.75,
        }
    }

    fn audio_input() -> AviQtlAudioBakeInput {
        AviQtlAudioBakeInput {
            clip_id: 8,
            start_frame: 11,
            duration_frames: 120,
            has_audio_effect: 1,
            fps: 60.0,
            source_start_time: -2.0,
            speed_percent: 150.0,
            direct_time: 3.0,
            volume: -1.0,
            master_volume: 0.5,
            pan: 2.0,
            fade_in_seconds: -4.0,
            fade_out_seconds: 5.0,
            direct_mode: 1,
            mute: 1,
            solo: 0,
            limiter: 1,
        }
    }

    #[test]
    fn builds_render_defaults_and_transform_state() {
        let mut output = AviQtlRenderBakeOutput::default();
        let input = render_input();
        assert_eq!(
            unsafe { aviqtl_timeline_bake_render(&input, &mut output) },
            STATUS_OK
        );
        assert_eq!(output.time_position, 15.0);
        assert_eq!(output.scale_x, 1.25);
        assert_eq!(output.opacity, 0.75);
        assert_eq!(output.effect_start_index, 9);

        let mut without_transform = input;
        without_transform.has_transform = 0;
        assert_eq!(
            unsafe { aviqtl_timeline_bake_render(&without_transform, &mut output) },
            STATUS_OK
        );
        assert_eq!(output.x, 0.0);
        assert_eq!(output.scale_x, 1.0);
        assert_eq!(output.opacity, 1.0);
    }

    #[test]
    fn builds_audio_defaults_and_clamps_effect_state() {
        let input = audio_input();
        let mut output = audio_defaults();
        assert_eq!(
            unsafe { aviqtl_timeline_bake_audio(&input, &mut output) },
            STATUS_OK
        );
        assert_eq!(output.clip_id, 8);
        assert_eq!(output.source_start_time, 0.0);
        assert_eq!(output.playback_speed, 1.5);
        assert_eq!(output.volume, 0.0);
        assert_eq!(output.pan, 1.0);
        assert_eq!(output.direct_mode, 1);

        let mut invalid_fps = input;
        invalid_fps.fps = 0.0;
        assert_eq!(
            unsafe { aviqtl_timeline_bake_audio(&invalid_fps, &mut output) },
            STATUS_OK
        );
        assert_eq!(output.clip_id, -1);
        assert_eq!(output.playback_speed, 1.0);
        assert_eq!(output.volume, 1.0);
    }

    #[test]
    fn rejects_null_misaligned_and_overlapping_ranges() {
        let input = render_input();
        let mut output = AviQtlRenderBakeOutput::default();
        assert_eq!(
            unsafe { aviqtl_timeline_bake_render(std::ptr::null(), &mut output) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { aviqtl_timeline_bake_render(&input, std::ptr::null_mut()) },
            STATUS_INVALID_ARGUMENT
        );

        let mut words = [0_u32; size_of::<AviQtlRenderBakeInput>().div_ceil(size_of::<u32>()) + 1];
        let misaligned = unsafe {
            words
                .as_mut_ptr()
                .cast::<u8>()
                .add(1)
                .cast::<AviQtlRenderBakeInput>()
        };
        assert_eq!(
            unsafe { aviqtl_timeline_bake_render(misaligned, &mut output) },
            STATUS_INVALID_ARGUMENT
        );

        let mut shared = MaybeUninit::<AviQtlRenderBakeOutput>::uninit();
        let shared_input = shared.as_mut_ptr().cast::<AviQtlRenderBakeInput>();
        unsafe { shared_input.write(input) };
        assert_eq!(
            unsafe { aviqtl_timeline_bake_render(shared_input, shared.as_mut_ptr()) },
            STATUS_OVERLAPPING_BUFFERS
        );
    }
}
