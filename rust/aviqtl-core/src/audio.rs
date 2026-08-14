use crate::abi::{
    AviQtlAudioMeter, AviQtlAudioMixParameters, STATUS_INVALID_ARGUMENT, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};

fn fade_gain(parameters: AviQtlAudioMixParameters) -> f32 {
    let mut gain: f64 = 1.0;
    if parameters.fade_in_seconds > 0.0 {
        gain = gain.min(
            (parameters.relative_time / f64::from(parameters.fade_in_seconds)).clamp(0.0, 1.0),
        );
    }
    if parameters.fade_out_seconds > 0.0 {
        gain = gain.min(
            ((parameters.duration - parameters.relative_time)
                / f64::from(parameters.fade_out_seconds))
            .clamp(0.0, 1.0),
        );
    }
    gain as f32
}

fn resample_stereo_linear(input: &[f32], output: &mut [f32], source_rate: f64) {
    let input_frames = input.len() / 2;
    for (frame, output_frame) in output.chunks_exact_mut(2).enumerate() {
        let source_index = frame as f64 * source_rate;
        let first_index = (source_index as usize).min(input_frames - 1);
        let second_index = first_index.saturating_add(1).min(input_frames - 1);
        let fraction = source_index - first_index as f64;

        output_frame[0] = (f64::from(input[first_index * 2]) * (1.0 - fraction)
            + f64::from(input[second_index * 2]) * fraction) as f32;
        output_frame[1] = (f64::from(input[first_index * 2 + 1]) * (1.0 - fraction)
            + f64::from(input[second_index * 2 + 1]) * fraction) as f32;
    }
}

fn mix_stereo(
    clip: &[f32],
    master: &mut [f32],
    parameters: AviQtlAudioMixParameters,
) -> AviQtlAudioMeter {
    let gain = parameters.volume * parameters.master_volume * fade_gain(parameters);
    let left_gain = gain
        * if parameters.pan <= 0.0 {
            1.0
        } else {
            1.0 - parameters.pan
        };
    let right_gain = gain
        * if parameters.pan >= 0.0 {
            1.0
        } else {
            1.0 + parameters.pan
        };

    let mut meter = AviQtlAudioMeter::default();
    let mut square_left = 0.0;
    let mut square_right = 0.0;
    for (clip_frame, master_frame) in clip.chunks_exact(2).zip(master.chunks_exact_mut(2)) {
        let mut left = clip_frame[0] * left_gain;
        let mut right = clip_frame[1] * right_gain;
        if parameters.limiter != 0 {
            left = left.clamp(-1.0, 1.0);
            right = right.clamp(-1.0, 1.0);
        }

        meter.peak_left = meter.peak_left.max(left.abs());
        meter.peak_right = meter.peak_right.max(right.abs());
        square_left += f64::from(left) * f64::from(left);
        square_right += f64::from(right) * f64::from(right);
        master_frame[0] += left;
        master_frame[1] += right;
    }

    let master_frames = master.len() / 2;
    if !clip.is_empty() && master_frames != 0 {
        let denominator = master_frames as f64;
        meter.rms_left = (square_left / denominator).sqrt() as f32;
        meter.rms_right = (square_right / denominator).sqrt() as f32;
    }
    meter
}

/// Linearly resamples an interleaved stereo buffer into caller-owned output storage.
///
/// # Safety
///
/// Non-zero lengths require aligned pointers to initialized `f32` elements. The output
/// pointer must be writable for `output_length` elements. Input and output ranges must
/// not overlap and must remain valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_audio_resample_stereo_linear(
    input: *const f32,
    input_length: usize,
    output: *mut f32,
    output_length: usize,
    source_rate: f64,
) -> u32 {
    if input_length % 2 != 0
        || output_length % 2 != 0
        || !source_rate.is_finite()
        || source_rate < 0.0
        || !slice_is_valid(input, input_length)
        || !slice_is_valid(output, output_length)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlaps) = ranges_overlap(input, input_length, output, output_length) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlaps {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    if output_length != 0 && input_length == 0 {
        return STATUS_INVALID_ARGUMENT;
    }

    let input = if input_length == 0 {
        &[]
    } else {
        // SAFETY: The pointer and length were validated above and the caller
        // guarantees that the allocation remains readable for the call.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    let output = if output_length == 0 {
        &mut []
    } else {
        // SAFETY: The pointer and length were validated above, and overlap with
        // the input range was rejected before constructing either slice.
        unsafe { std::slice::from_raw_parts_mut(output, output_length) }
    };
    resample_stereo_linear(input, output, source_rate);
    STATUS_OK
}

/// Mixes one interleaved stereo clip into a caller-owned master buffer and reports levels.
///
/// # Safety
///
/// Non-zero buffer lengths require aligned pointers to initialized `f32` elements. The
/// master range and meter must be writable. Clip, master, and meter ranges must not overlap
/// and must remain valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_audio_mix_stereo(
    clip: *const f32,
    clip_length: usize,
    master: *mut f32,
    master_length: usize,
    parameters: AviQtlAudioMixParameters,
    meter: *mut AviQtlAudioMeter,
) -> u32 {
    if clip_length % 2 != 0
        || master_length % 2 != 0
        || !slice_is_valid(clip, clip_length)
        || !slice_is_valid(master, master_length)
        || !slice_is_valid(meter, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }

    let overlaps = [
        ranges_overlap(clip, clip_length, master, master_length),
        ranges_overlap(clip, clip_length, meter, 1),
        ranges_overlap(master, master_length, meter, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }

    let clip = if clip_length == 0 {
        &[]
    } else {
        // SAFETY: The pointer and length were validated above and all mutable
        // output ranges were checked for overlap before constructing the slice.
        unsafe { std::slice::from_raw_parts(clip, clip_length) }
    };
    let master = if master_length == 0 {
        &mut []
    } else {
        // SAFETY: The pointer and length were validated above and all other
        // ranges were checked for overlap before constructing the mutable slice.
        unsafe { std::slice::from_raw_parts_mut(master, master_length) }
    };
    let result = mix_stereo(clip, master, parameters);
    // SAFETY: The meter pointer was validated and checked against both buffer ranges.
    unsafe { meter.write(result) };
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "actual={actual:.9e} expected={expected:.9e}"
        );
    }

    fn mix_parameters() -> AviQtlAudioMixParameters {
        AviQtlAudioMixParameters {
            relative_time: 0.25,
            duration: 1.0,
            fade_in_seconds: 0.5,
            fade_out_seconds: 0.0,
            volume: 2.0,
            master_volume: 0.5,
            pan: 0.5,
            limiter: 1,
        }
    }

    #[test]
    fn resampling_matches_cpp_behavior() {
        let input = [0.25, -0.5, 0.75, 0.5, -1.0, 1.0, 0.5, -0.25];
        let mut output = [0.0; 10];
        // SAFETY: Both arrays are valid, aligned, non-overlapping stereo buffers.
        let status = unsafe {
            aviqtl_audio_resample_stereo_linear(
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                0.75,
            )
        };
        assert_eq!(status, STATUS_OK);
        let expected = [
            0.25, -0.5, 0.625, 0.25, -0.125, 0.75, -0.625, 0.6875, 0.5, -0.25,
        ];
        for (actual, expected) in output.into_iter().zip(expected) {
            assert_close(actual, expected);
        }
    }

    #[test]
    fn mixing_matches_cpp_behavior() {
        let clip = [0.5, -0.5, 2.0, -2.0];
        let mut master = [0.1, 0.2, -0.25, 0.25];
        let mut meter = AviQtlAudioMeter::default();
        // SAFETY: The arrays and meter are valid, aligned, and non-overlapping.
        let status = unsafe {
            aviqtl_audio_mix_stereo(
                clip.as_ptr(),
                clip.len(),
                master.as_mut_ptr(),
                master.len(),
                mix_parameters(),
                &mut meter,
            )
        };
        assert_eq!(status, STATUS_OK);
        for (actual, expected) in master.into_iter().zip([0.225, -0.05, 0.25, -0.75]) {
            assert_close(actual, expected);
        }
        assert_close(meter.peak_left, 0.5);
        assert_close(meter.peak_right, 1.0);
        assert_close(meter.rms_left, 0.364_434_48);
        assert_close(meter.rms_right, 0.728_868_96);
    }

    #[test]
    fn ffi_rejects_invalid_audio_buffers() {
        let mut stereo = [0.0_f32; 4];
        let mut meter = AviQtlAudioMeter::default();
        // SAFETY: These calls deliberately provide invalid combinations which
        // must be rejected before the boundary constructs slices.
        unsafe {
            assert_eq!(
                aviqtl_audio_resample_stereo_linear(
                    std::ptr::null(),
                    2,
                    stereo.as_mut_ptr(),
                    stereo.len(),
                    1.0,
                ),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(
                aviqtl_audio_resample_stereo_linear(
                    std::ptr::null(),
                    0,
                    stereo.as_mut_ptr(),
                    stereo.len(),
                    1.0,
                ),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(
                aviqtl_audio_resample_stereo_linear(
                    stereo.as_ptr(),
                    stereo.len(),
                    stereo.as_mut_ptr(),
                    stereo.len(),
                    1.0,
                ),
                STATUS_OVERLAPPING_BUFFERS
            );
            assert_eq!(
                aviqtl_audio_resample_stereo_linear(
                    stereo.as_ptr(),
                    stereo.len(),
                    stereo.as_mut_ptr(),
                    0,
                    f64::NAN,
                ),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(
                aviqtl_audio_mix_stereo(
                    stereo.as_ptr(),
                    3,
                    stereo.as_mut_ptr(),
                    0,
                    mix_parameters(),
                    &mut meter,
                ),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(
                aviqtl_audio_mix_stereo(
                    stereo.as_ptr(),
                    stereo.len(),
                    stereo.as_mut_ptr(),
                    stereo.len(),
                    mix_parameters(),
                    &mut meter,
                ),
                STATUS_OVERLAPPING_BUFFERS
            );
            assert_eq!(
                aviqtl_audio_mix_stereo(
                    stereo.as_ptr(),
                    stereo.len(),
                    std::ptr::null_mut(),
                    0,
                    mix_parameters(),
                    std::ptr::null_mut(),
                ),
                STATUS_INVALID_ARGUMENT
            );
        }
    }

    #[test]
    fn empty_audio_buffers_are_valid() {
        let mut meter = AviQtlAudioMeter {
            peak_left: 1.0,
            peak_right: 1.0,
            rms_left: 1.0,
            rms_right: 1.0,
        };
        // SAFETY: Zero lengths permit null buffer pointers, while meter is valid.
        let status = unsafe {
            aviqtl_audio_mix_stereo(
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                mix_parameters(),
                &mut meter,
            )
        };
        assert_eq!(status, STATUS_OK);
        assert_close(meter.peak_left, 0.0);
        assert_close(meter.peak_right, 0.0);
        assert_close(meter.rms_left, 0.0);
        assert_close(meter.rms_right, 0.0);
    }
}
