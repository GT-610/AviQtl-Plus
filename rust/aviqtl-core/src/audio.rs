use crate::abi::{
    AviQtlAudioBatchResult, AviQtlAudioBatchTrack, AviQtlAudioMeter, AviQtlAudioMixParameters,
    STATUS_INVALID_ARGUMENT, STATUS_OK, STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
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
    for (frame, output_frame) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
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
    for (clip_frame, master_frame) in clip
        .as_chunks::<2>()
        .0
        .iter()
        .zip(master.as_chunks_mut::<2>().0.iter_mut())
    {
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
    if !input_length.is_multiple_of(2)
        || !output_length.is_multiple_of(2)
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

/// Mixes multiple interleaved stereo clips into one caller-owned master buffer.
///
/// Solo and mute selection is evaluated for the complete batch. One result is
/// written per input track, including a zero meter for tracks that were skipped.
///
/// # Safety
///
/// Non-zero lengths require aligned, initialized input ranges and writable output
/// ranges. Track descriptors, nested sample buffers, the master buffer, and results
/// must remain valid and must not overlap any mutable range for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_audio_mix_stereo_batch(
    tracks: *const AviQtlAudioBatchTrack,
    tracks_length: usize,
    master: *mut f32,
    master_length: usize,
    results: *mut AviQtlAudioBatchResult,
    results_length: usize,
) -> u32 {
    if tracks_length != results_length
        || !master_length.is_multiple_of(2)
        || !slice_is_valid(tracks, tracks_length)
        || !slice_is_valid(master, master_length)
        || !slice_is_valid(results, results_length)
    {
        return STATUS_INVALID_ARGUMENT;
    }

    let top_level_overlaps = [
        ranges_overlap(tracks, tracks_length, master, master_length),
        ranges_overlap(tracks, tracks_length, results, results_length),
        ranges_overlap(master, master_length, results, results_length),
    ];
    if top_level_overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if top_level_overlaps
        .into_iter()
        .flatten()
        .any(|overlap| overlap)
    {
        return STATUS_OVERLAPPING_BUFFERS;
    }

    let tracks = if tracks_length == 0 {
        &[]
    } else {
        // SAFETY: The descriptor range was validated and checked against both outputs.
        unsafe { std::slice::from_raw_parts(tracks, tracks_length) }
    };
    for track in tracks {
        if !track.samples_length.is_multiple_of(2)
            || !slice_is_valid(track.samples, track.samples_length)
        {
            return STATUS_INVALID_ARGUMENT;
        }
        let nested_overlaps = [
            ranges_overlap(
                track.samples,
                track.samples_length,
                tracks.as_ptr(),
                tracks.len(),
            ),
            ranges_overlap(track.samples, track.samples_length, master, master_length),
            ranges_overlap(track.samples, track.samples_length, results, results_length),
        ];
        if nested_overlaps.iter().any(Option::is_none) {
            return STATUS_INVALID_ARGUMENT;
        }
        if nested_overlaps.into_iter().flatten().any(|overlap| overlap) {
            return STATUS_OVERLAPPING_BUFFERS;
        }
    }

    let has_solo = tracks
        .iter()
        .any(|track| track.solo != 0 && track.mute == 0);
    let master = if master_length == 0 {
        &mut []
    } else {
        // SAFETY: The master range was validated and checked against every input/output.
        unsafe { std::slice::from_raw_parts_mut(master, master_length) }
    };
    let results = if results_length == 0 {
        &mut []
    } else {
        // SAFETY: The result range was validated and checked against every input/output.
        unsafe { std::slice::from_raw_parts_mut(results, results_length) }
    };

    for (track, result) in tracks.iter().zip(results) {
        let mut output = AviQtlAudioBatchResult {
            clip_id: track.clip_id,
            ..AviQtlAudioBatchResult::default()
        };
        if track.mute == 0 && (!has_solo || track.solo != 0) {
            let samples = if track.samples_length == 0 {
                &[]
            } else {
                // SAFETY: This nested range was validated and checked against outputs above.
                unsafe { std::slice::from_raw_parts(track.samples, track.samples_length) }
            };
            output.mixed = 1;
            output.meter = mix_stereo(samples, master, track.parameters);
        }
        *result = output;
    }
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
    fn resampling_rejects_invalid_audio_buffers() {
        let mut stereo = [0.0_f32; 4];
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
        }
    }

    #[test]
    fn batch_mixing_applies_solo_mute_and_reports_each_track() {
        let solo_samples = [0.5_f32, -0.5, 0.25, -0.25];
        let skipped_samples = [1.0_f32, 1.0, 1.0, 1.0];
        let tracks = [
            AviQtlAudioBatchTrack {
                samples: solo_samples.as_ptr(),
                samples_length: solo_samples.len(),
                parameters: AviQtlAudioMixParameters {
                    relative_time: 0.0,
                    duration: 1.0,
                    fade_in_seconds: 0.0,
                    fade_out_seconds: 0.0,
                    volume: 1.0,
                    master_volume: 1.0,
                    pan: 0.0,
                    limiter: 0,
                },
                clip_id: 10,
                mute: 0,
                solo: 1,
                reserved: 0,
            },
            AviQtlAudioBatchTrack {
                samples: skipped_samples.as_ptr(),
                samples_length: skipped_samples.len(),
                parameters: mix_parameters(),
                clip_id: 11,
                mute: 0,
                solo: 0,
                reserved: 0,
            },
            AviQtlAudioBatchTrack {
                samples: skipped_samples.as_ptr(),
                samples_length: skipped_samples.len(),
                parameters: mix_parameters(),
                clip_id: 12,
                mute: 1,
                solo: 1,
                reserved: 0,
            },
        ];
        let mut master = [0.0_f32; 4];
        let mut results = [AviQtlAudioBatchResult::default(); 3];
        // SAFETY: All descriptor, nested input, and output ranges are valid and disjoint.
        let status = unsafe {
            aviqtl_audio_mix_stereo_batch(
                tracks.as_ptr(),
                tracks.len(),
                master.as_mut_ptr(),
                master.len(),
                results.as_mut_ptr(),
                results.len(),
            )
        };
        assert_eq!(status, STATUS_OK);
        assert_eq!(master, solo_samples);
        assert_eq!(results[0].clip_id, 10);
        assert_eq!(results[0].mixed, 1);
        assert_close(results[0].meter.peak_left, 0.5);
        assert_eq!(results[1].clip_id, 11);
        assert_eq!(results[1].mixed, 0);
        assert_close(results[1].meter.peak_left, 0.0);
        assert_eq!(results[2].clip_id, 12);
        assert_eq!(results[2].mixed, 0);
    }

    #[test]
    fn batch_mixing_rejects_nested_output_overlap_without_writes() {
        let mut master = [0.25_f32, -0.25, 0.5, -0.5];
        let valid_samples = [1.0_f32, 1.0, 1.0, 1.0];
        let tracks = [
            AviQtlAudioBatchTrack {
                samples: valid_samples.as_ptr(),
                samples_length: valid_samples.len(),
                parameters: mix_parameters(),
                clip_id: 6,
                mute: 0,
                solo: 0,
                reserved: 0,
            },
            AviQtlAudioBatchTrack {
                samples: master.as_ptr(),
                samples_length: master.len(),
                parameters: mix_parameters(),
                clip_id: 7,
                mute: 0,
                solo: 0,
                reserved: 0,
            },
        ];
        let mut results = [
            AviQtlAudioBatchResult {
                clip_id: 98,
                mixed: 98,
                meter: AviQtlAudioMeter::default(),
            },
            AviQtlAudioBatchResult {
                clip_id: 99,
                mixed: 99,
                meter: AviQtlAudioMeter::default(),
            },
        ];
        let original_master = master;
        // SAFETY: The second track's deliberate overlap must reject the whole batch
        // before the valid first track can modify either output.
        let status = unsafe {
            aviqtl_audio_mix_stereo_batch(
                tracks.as_ptr(),
                tracks.len(),
                master.as_mut_ptr(),
                master.len(),
                results.as_mut_ptr(),
                results.len(),
            )
        };
        assert_eq!(status, STATUS_OVERLAPPING_BUFFERS);
        assert_eq!(master, original_master);
        assert_eq!(results[0].clip_id, 98);
        assert_eq!(results[0].mixed, 98);
        assert_eq!(results[1].clip_id, 99);
        assert_eq!(results[1].mixed, 99);
    }
}
