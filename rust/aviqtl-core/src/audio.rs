use crate::abi::{
    AviQtlAudioBatchResult, AviQtlAudioBatchTrack, AviQtlAudioMeter, AviQtlAudioMixParameters,
    AviQtlAudioPlaybackContext, AviQtlAudioPlaybackInput, AviQtlAudioPlaybackPlan,
    AviQtlWaveformContext, AviQtlWaveformEvaluatedPoint, AviQtlWaveformPlan,
    AviQtlWaveformSamplingPoint, STATUS_INVALID_ARGUMENT, STATUS_OK, STATUS_OVERLAPPING_BUFFERS,
    ranges_overlap, slice_is_valid,
};

const DEFAULT_SPEED_PERCENT: f64 = 100.0;
const PLAYBACK_ACTION_SKIP: u32 = 0;
const PLAYBACK_ACTION_SILENCE: u32 = 1;
const PLAYBACK_ACTION_FETCH_DIRECT: u32 = 2;
const PLAYBACK_ACTION_FETCH_RESAMPLE: u32 = 3;

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

fn ranges_are_disjoint<T, U>(
    first: *const T,
    first_length: usize,
    second: *const U,
    second_length: usize,
) -> Result<(), u32> {
    match ranges_overlap(first, first_length, second, second_length) {
        Some(false) => Ok(()),
        Some(true) => Err(STATUS_OVERLAPPING_BUFFERS),
        None => Err(STATUS_INVALID_ARGUMENT),
    }
}

fn active_at_frame(input: AviQtlAudioPlaybackInput, current_frame: i32) -> bool {
    let current_frame = i64::from(current_frame);
    let start_frame = i64::from(input.start_frame);
    let end_frame = start_frame + i64::from(input.duration_frames);
    current_frame >= start_frame && current_frame < end_frame
}

fn silence_plan(input: AviQtlAudioPlaybackInput, report_meter: bool) -> AviQtlAudioPlaybackPlan {
    AviQtlAudioPlaybackPlan {
        clip_id: input.clip_id,
        action: PLAYBACK_ACTION_SILENCE,
        report_meter: u32::from(report_meter),
        mute: u32::from(input.mute != 0),
        solo: u32::from(input.solo != 0),
        ..AviQtlAudioPlaybackPlan::default()
    }
}

fn plan_playback_batch(
    context: AviQtlAudioPlaybackContext,
    inputs: &[AviQtlAudioPlaybackInput],
) -> Option<Vec<AviQtlAudioPlaybackPlan>> {
    if !context.fps.is_finite()
        || context.fps <= 0.0
        || !context.mixer_playback_speed.is_finite()
        || context.samples_per_frame < 0
        || context.samples_per_frame > i32::MAX / 2
        || context.sample_rate <= 0
    {
        return None;
    }

    let has_solo = inputs.iter().copied().any(|input| {
        input.solo != 0 && input.mute == 0 && active_at_frame(input, context.current_frame)
    });
    let mut output = Vec::with_capacity(inputs.len());
    for input in inputs.iter().copied() {
        if input.mute != 0 || (has_solo && input.solo == 0) {
            output.push(silence_plan(input, true));
            continue;
        }
        if !active_at_frame(input, context.current_frame) {
            output.push(AviQtlAudioPlaybackPlan {
                clip_id: input.clip_id,
                action: PLAYBACK_ACTION_SKIP,
                ..AviQtlAudioPlaybackPlan::default()
            });
            continue;
        }
        if input.decoder_available == 0 {
            output.push(silence_plan(input, false));
            continue;
        }

        let relative_time = f64::from(context.current_frame) - f64::from(input.start_frame);
        let relative_time = relative_time / context.fps;
        let direct_mode = input.direct_mode != 0;
        let mut source_start_time = if direct_mode {
            input.direct_time
        } else {
            input.source_start_time + relative_time * input.playback_speed
        };
        let consecutive = !direct_mode
            && input.has_previous_phase != 0
            && input.previous_frame.checked_add(1) == Some(context.current_frame);
        if consecutive {
            source_start_time = input.previous_phase;
        }

        let source_rate = (context.mixer_playback_speed
            * if direct_mode {
                1.0
            } else {
                input.playback_speed
            })
        .max(0.0);
        if !source_start_time.is_finite() || !source_rate.is_finite() {
            return None;
        }
        let resample = (source_rate - 1.0).abs() > 0.01;
        let source_sample_count = if resample {
            let required = (f64::from(context.samples_per_frame) * source_rate).ceil();
            if required > f64::from(i32::MAX / 2 - 2) {
                return None;
            }
            required as i32 + 2
        } else {
            context.samples_per_frame
        };
        let phase_step = if resample { source_rate } else { 1.0 };
        let next_phase = source_start_time
            + (f64::from(context.samples_per_frame) / f64::from(context.sample_rate)) * phase_step;
        if !next_phase.is_finite() {
            return None;
        }

        output.push(AviQtlAudioPlaybackPlan {
            clip_id: input.clip_id,
            action: if resample {
                PLAYBACK_ACTION_FETCH_RESAMPLE
            } else {
                PLAYBACK_ACTION_FETCH_DIRECT
            },
            source_start_time,
            source_rate,
            next_phase,
            source_sample_count,
            report_meter: 1,
            mute: 0,
            solo: u32::from(input.solo != 0),
            parameters: AviQtlAudioMixParameters {
                relative_time,
                duration: f64::from(input.duration_frames) / context.fps,
                fade_in_seconds: input.fade_in_seconds,
                fade_out_seconds: input.fade_out_seconds,
                volume: input.volume,
                master_volume: input.master_volume,
                pan: input.pan,
                limiter: u32::from(input.limiter != 0),
            },
        });
    }
    Some(output)
}

fn waveform_sampling_point(
    pixel: i32,
    pixel_width: i32,
    display_duration_frames: i32,
) -> AviQtlWaveformSamplingPoint {
    let width = f64::from(pixel_width);
    let duration = f64::from(display_duration_frames);
    let relative_frame = (duration * f64::from(pixel) / width)
        .floor()
        .clamp(0.0, f64::from(display_duration_frames - 1)) as i32;
    let next_relative_frame = (duration * f64::from(pixel + 1) / width)
        .ceil()
        .clamp(f64::from(relative_frame + 1), duration) as i32;
    AviQtlWaveformSamplingPoint {
        relative_frame,
        next_relative_frame,
    }
}

fn plan_waveform(
    context: AviQtlWaveformContext,
    evaluated: &[AviQtlWaveformEvaluatedPoint],
) -> Option<Vec<AviQtlWaveformPlan>> {
    if context.pixel_width <= 0
        || context.display_duration_frames <= 0
        || !context.fps.is_finite()
        || context.fps <= 0.0
        || evaluated.len() != context.pixel_width as usize
    {
        return None;
    }

    let frame_step_seconds = 1.0 / context.fps;
    let clip_duration_seconds = f64::from(context.display_duration_frames) / context.fps;
    let has_audio_effect = context.has_audio_effect != 0;
    let direct_mode = context.direct_mode != 0;
    let linked_video = context.linked_video != 0;
    let mut output = Vec::with_capacity(evaluated.len());
    for (pixel, evaluated) in evaluated.iter().copied().enumerate() {
        let point = waveform_sampling_point(
            pixel as i32,
            context.pixel_width,
            context.display_duration_frames,
        );
        let relative_seconds = f64::from(point.relative_frame) / context.fps;
        let next_relative_seconds = f64::from(point.next_relative_frame) / context.fps;

        let (source_start_seconds, source_duration_seconds) = if !has_audio_effect {
            (
                relative_seconds,
                (clip_duration_seconds / f64::from(context.pixel_width)).max(frame_step_seconds),
            )
        } else if direct_mode {
            (
                evaluated.direct_time.min(evaluated.next_direct_time),
                (evaluated.next_direct_time - evaluated.direct_time)
                    .abs()
                    .max(frame_step_seconds),
            )
        } else {
            let start_time = evaluated.start_time.max(0.0);
            let speed_percent = if linked_video {
                DEFAULT_SPEED_PERCENT
            } else {
                evaluated.speed_percent
            };
            let source_rate = (speed_percent / DEFAULT_SPEED_PERCENT).max(0.0);
            (
                start_time + relative_seconds * source_rate,
                ((next_relative_seconds - relative_seconds) * source_rate).max(frame_step_seconds),
            )
        };

        let (volume, master_volume, pan, fade_in, fade_out, muted) = if has_audio_effect {
            (
                evaluated.volume.max(0.0),
                evaluated.master_volume.max(0.0),
                evaluated.pan.clamp(-1.0, 1.0),
                evaluated.fade_in_seconds.max(0.0),
                evaluated.fade_out_seconds.max(0.0),
                evaluated.mute != 0,
            )
        } else {
            (1.0, 1.0, 0.0, 0.0, 0.0, false)
        };
        let mut fade_gain: f64 = 1.0;
        if fade_in > 0.0 {
            fade_gain = fade_gain.min((relative_seconds / fade_in).clamp(0.0, 1.0));
        }
        if fade_out > 0.0 {
            fade_gain = fade_gain
                .min(((clip_duration_seconds - relative_seconds) / fade_out).clamp(0.0, 1.0));
        }
        let output_volume = volume * master_volume * fade_gain;
        let left_volume = if muted {
            0.0
        } else {
            output_volume * if pan <= 0.0 { 1.0 } else { 1.0 - pan }
        };
        let right_volume = if muted {
            0.0
        } else {
            output_volume * if pan >= 0.0 { 1.0 } else { 1.0 + pan }
        };
        output.push(AviQtlWaveformPlan {
            source_start_seconds,
            source_duration_seconds,
            display_gain: ((left_volume + right_volume) * 0.5).clamp(0.0, 2.0) as f32,
            reserved: 0,
        });
    }
    Some(output)
}

/// Plans decoder and resampling actions for a complete ECS audio batch.
///
/// # Safety
///
/// All non-empty ranges must be aligned and valid for the duration of the call. The context,
/// input, and output ranges must be pairwise disjoint. The output length must equal the input
/// length. No output is written when validation or planning fails.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_audio_plan_playback_batch(
    context: *const AviQtlAudioPlaybackContext,
    inputs: *const AviQtlAudioPlaybackInput,
    inputs_length: usize,
    output: *mut AviQtlAudioPlaybackPlan,
    output_length: usize,
) -> u32 {
    if inputs_length != output_length
        || !slice_is_valid(context, 1)
        || !slice_is_valid(inputs, inputs_length)
        || !slice_is_valid(output, output_length)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    for result in [
        ranges_are_disjoint(context, 1, inputs, inputs_length),
        ranges_are_disjoint(context, 1, output, output_length),
        ranges_are_disjoint(inputs, inputs_length, output, output_length),
    ] {
        if let Err(status) = result {
            return status;
        }
    }

    // SAFETY: All ranges were validated as aligned, valid, and pairwise disjoint above.
    let context = unsafe { context.read() };
    let inputs = if inputs_length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(inputs, inputs_length) }
    };
    let Some(plans) = plan_playback_batch(context, inputs) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if output_length != 0 {
        // SAFETY: The output range was validated and is disjoint from every input range.
        unsafe { std::ptr::copy_nonoverlapping(plans.as_ptr(), output, output_length) };
    }
    STATUS_OK
}

/// Maps waveform pixels to the relative frames whose evaluated parameters are required.
///
/// # Safety
///
/// `context` must point to one initialized value and `output` to `output_length` writable values.
/// The ranges must be disjoint. The output length must equal `context.pixel_width`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_audio_waveform_sampling_points(
    context: *const AviQtlWaveformContext,
    output: *mut AviQtlWaveformSamplingPoint,
    output_length: usize,
) -> u32 {
    if !slice_is_valid(context, 1) || !slice_is_valid(output, output_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    if let Err(status) = ranges_are_disjoint(context, 1, output, output_length) {
        return status;
    }
    // SAFETY: The context range was validated and does not overlap the output.
    let context = unsafe { context.read() };
    if context.pixel_width <= 0
        || context.display_duration_frames <= 0
        || output_length != context.pixel_width as usize
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let points: Vec<_> = (0..context.pixel_width)
        .map(|pixel| {
            waveform_sampling_point(pixel, context.pixel_width, context.display_duration_frames)
        })
        .collect();
    // SAFETY: A positive pixel width requires a non-null writable output range of equal length.
    unsafe { std::ptr::copy_nonoverlapping(points.as_ptr(), output, output_length) };
    STATUS_OK
}

/// Builds decoder peak ranges and display gains from already evaluated effect parameters.
///
/// # Safety
///
/// All non-empty ranges must be aligned, valid, and pairwise disjoint. Input and output lengths
/// must equal `context.pixel_width`. No output is written when validation or planning fails.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_audio_plan_waveform(
    context: *const AviQtlWaveformContext,
    evaluated: *const AviQtlWaveformEvaluatedPoint,
    evaluated_length: usize,
    output: *mut AviQtlWaveformPlan,
    output_length: usize,
) -> u32 {
    if evaluated_length != output_length
        || !slice_is_valid(context, 1)
        || !slice_is_valid(evaluated, evaluated_length)
        || !slice_is_valid(output, output_length)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    for result in [
        ranges_are_disjoint(context, 1, evaluated, evaluated_length),
        ranges_are_disjoint(context, 1, output, output_length),
        ranges_are_disjoint(evaluated, evaluated_length, output, output_length),
    ] {
        if let Err(status) = result {
            return status;
        }
    }

    // SAFETY: All ranges were validated as aligned, valid, and pairwise disjoint above.
    let context = unsafe { context.read() };
    let evaluated = if evaluated_length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(evaluated, evaluated_length) }
    };
    let Some(plans) = plan_waveform(context, evaluated) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if output_length != 0 {
        // SAFETY: The output range was validated and is disjoint from every input range.
        unsafe { std::ptr::copy_nonoverlapping(plans.as_ptr(), output, output_length) };
    }
    STATUS_OK
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

    fn assert_close_f64(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "actual={actual:.12e} expected={expected:.12e}"
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

    fn playback_input(clip_id: i32) -> AviQtlAudioPlaybackInput {
        AviQtlAudioPlaybackInput {
            clip_id,
            start_frame: 10,
            duration_frames: 10,
            source_start_time: 1.0,
            playback_speed: 1.0,
            volume: 1.0,
            master_volume: 1.0,
            decoder_available: 1,
            ..AviQtlAudioPlaybackInput::default()
        }
    }

    #[test]
    fn playback_planning_owns_solo_selection_resampling_and_continuous_phase() {
        let context = AviQtlAudioPlaybackContext {
            current_frame: 11,
            samples_per_frame: 480,
            sample_rate: 48_000,
            fps: 60.0,
            mixer_playback_speed: 1.5,
            ..AviQtlAudioPlaybackContext::default()
        };
        let mut solo = playback_input(10);
        solo.solo = 1;
        solo.previous_frame = 10;
        solo.previous_phase = 5.0;
        solo.has_previous_phase = 1;
        let normal = playback_input(11);
        let mut muted_outside = playback_input(12);
        muted_outside.start_frame = 100;
        muted_outside.mute = 1;
        let mut missing_decoder = playback_input(13);
        missing_decoder.solo = 1;
        missing_decoder.decoder_available = 0;
        let inputs = [solo, normal, muted_outside, missing_decoder];

        let plans = plan_playback_batch(context, &inputs).expect("valid playback plan");
        assert_eq!(plans.len(), inputs.len());
        assert_eq!(plans[0].action, PLAYBACK_ACTION_FETCH_RESAMPLE);
        assert_close_f64(plans[0].source_start_time, 5.0);
        assert_close_f64(plans[0].source_rate, 1.5);
        assert_eq!(plans[0].source_sample_count, 722);
        assert_close_f64(plans[0].next_phase, 5.015);
        assert_close_f64(plans[0].parameters.relative_time, 1.0 / 60.0);
        assert_eq!(plans[0].report_meter, 1);

        assert_eq!(plans[1].action, PLAYBACK_ACTION_SILENCE);
        assert_eq!(plans[1].report_meter, 1);
        assert_eq!(plans[1].mute, 0);
        assert_eq!(plans[2].action, PLAYBACK_ACTION_SILENCE);
        assert_eq!(plans[2].mute, 1);
        assert_eq!(plans[2].report_meter, 1);
        assert_eq!(plans[3].action, PLAYBACK_ACTION_SILENCE);
        assert_eq!(plans[3].report_meter, 0);
        assert_eq!(plans[3].solo, 1);
    }

    #[test]
    fn direct_playback_ignores_previous_phase_and_uses_direct_fetch_threshold() {
        let context = AviQtlAudioPlaybackContext {
            current_frame: 12,
            samples_per_frame: 480,
            sample_rate: 48_000,
            fps: 60.0,
            mixer_playback_speed: 1.005,
            ..AviQtlAudioPlaybackContext::default()
        };
        let mut input = playback_input(20);
        input.direct_mode = 1;
        input.direct_time = 7.0;
        input.previous_frame = 11;
        input.previous_phase = 99.0;
        input.has_previous_phase = 1;

        let plans = plan_playback_batch(context, &[input]).expect("valid direct plan");
        assert_eq!(plans[0].action, PLAYBACK_ACTION_FETCH_DIRECT);
        assert_close_f64(plans[0].source_start_time, 7.0);
        assert_close_f64(plans[0].source_rate, 1.005);
        assert_eq!(plans[0].source_sample_count, 480);
        assert_close_f64(plans[0].next_phase, 7.01);
    }

    #[test]
    fn waveform_planning_maps_pixels_and_applies_time_gain_and_linked_video_rules() {
        let sampling_context = AviQtlWaveformContext {
            pixel_width: 3,
            display_duration_frames: 10,
            fps: 10.0,
            ..AviQtlWaveformContext::default()
        };
        let points: Vec<_> = (0..3)
            .map(|pixel| waveform_sampling_point(pixel, 3, 10))
            .collect();
        assert_eq!(points[0].relative_frame, 0);
        assert_eq!(points[0].next_relative_frame, 4);
        assert_eq!(points[1].relative_frame, 3);
        assert_eq!(points[1].next_relative_frame, 7);
        assert_eq!(points[2].relative_frame, 6);
        assert_eq!(points[2].next_relative_frame, 10);

        let normal_context = AviQtlWaveformContext {
            pixel_width: 2,
            display_duration_frames: 10,
            fps: sampling_context.fps,
            has_audio_effect: 1,
            linked_video: 1,
            ..AviQtlWaveformContext::default()
        };
        let evaluated = [
            AviQtlWaveformEvaluatedPoint {
                start_time: 1.0,
                speed_percent: 400.0,
                volume: 2.0,
                master_volume: 0.5,
                pan: 0.5,
                fade_in_seconds: 1.0,
                ..AviQtlWaveformEvaluatedPoint::default()
            },
            AviQtlWaveformEvaluatedPoint {
                start_time: 1.0,
                speed_percent: 400.0,
                volume: 2.0,
                master_volume: 0.5,
                pan: 0.5,
                fade_in_seconds: 1.0,
                ..AviQtlWaveformEvaluatedPoint::default()
            },
        ];
        let normal = plan_waveform(normal_context, &evaluated).expect("valid waveform plan");
        assert_close_f64(normal[0].source_start_seconds, 1.0);
        assert_close_f64(normal[0].source_duration_seconds, 0.5);
        assert_close(normal[0].display_gain, 0.0);
        assert_close_f64(normal[1].source_start_seconds, 1.5);
        assert_close(normal[1].display_gain, 0.375);

        let direct_context = AviQtlWaveformContext {
            direct_mode: 1,
            ..normal_context
        };
        let direct_values = [
            AviQtlWaveformEvaluatedPoint {
                direct_time: 2.0,
                next_direct_time: 1.25,
                volume: 1.0,
                master_volume: 1.0,
                ..AviQtlWaveformEvaluatedPoint::default()
            },
            AviQtlWaveformEvaluatedPoint {
                direct_time: 1.25,
                next_direct_time: 1.25,
                volume: 1.0,
                master_volume: 1.0,
                ..AviQtlWaveformEvaluatedPoint::default()
            },
        ];
        let direct = plan_waveform(direct_context, &direct_values).expect("valid direct waveform");
        assert_close_f64(direct[0].source_start_seconds, 1.25);
        assert_close_f64(direct[0].source_duration_seconds, 0.75);
        assert_close_f64(direct[1].source_duration_seconds, 0.1);

        let plain_context = AviQtlWaveformContext {
            has_audio_effect: 0,
            direct_mode: 0,
            linked_video: 0,
            ..normal_context
        };
        let plain =
            plan_waveform(plain_context, &[Default::default(); 2]).expect("valid plain waveform");
        assert_close_f64(plain[1].source_start_seconds, 0.5);
        assert_close_f64(plain[1].source_duration_seconds, 0.5);
        assert_close(plain[1].display_gain, 1.0);
    }

    #[test]
    fn planning_ffi_rejects_wrong_lengths_without_partial_writes() {
        let context = AviQtlAudioPlaybackContext {
            current_frame: 10,
            samples_per_frame: 480,
            sample_rate: 48_000,
            fps: 60.0,
            mixer_playback_speed: 1.0,
            ..AviQtlAudioPlaybackContext::default()
        };
        let input = playback_input(30);
        let mut output = AviQtlAudioPlaybackPlan {
            clip_id: 999,
            action: 999,
            ..AviQtlAudioPlaybackPlan::default()
        };
        // SAFETY: The ranges are valid and disjoint; the deliberately wrong output length
        // must be rejected before the sentinel output can be modified.
        let status =
            unsafe { aviqtl_audio_plan_playback_batch(&context, &input, 1, &mut output, 0) };
        assert_eq!(status, STATUS_INVALID_ARGUMENT);
        assert_eq!(output.clip_id, 999);
        assert_eq!(output.action, 999);

        let waveform_context = AviQtlWaveformContext {
            pixel_width: 2,
            display_duration_frames: 10,
            fps: 60.0,
            ..AviQtlWaveformContext::default()
        };
        let mut sampling_output = AviQtlWaveformSamplingPoint {
            relative_frame: 77,
            next_relative_frame: 88,
        };
        // SAFETY: The single output value is valid, but its length disagrees with pixel_width.
        let status = unsafe {
            aviqtl_audio_waveform_sampling_points(&waveform_context, &mut sampling_output, 1)
        };
        assert_eq!(status, STATUS_INVALID_ARGUMENT);
        assert_eq!(sampling_output.relative_frame, 77);
        assert_eq!(sampling_output.next_relative_frame, 88);

        let mut overlapping_input = playback_input(31);
        let original_input = overlapping_input;
        let input_pointer = std::ptr::addr_of!(overlapping_input);
        let overlapping_output = std::ptr::addr_of_mut!(overlapping_input).cast();
        // SAFETY: The output deliberately aliases the larger aligned input allocation. The
        // boundary must detect that overlap before constructing slices or writing output.
        let status = unsafe {
            aviqtl_audio_plan_playback_batch(&context, input_pointer, 1, overlapping_output, 1)
        };
        assert_eq!(status, STATUS_OVERLAPPING_BUFFERS);
        assert_eq!(overlapping_input.clip_id, original_input.clip_id);
        assert_eq!(
            overlapping_input.source_start_time,
            original_input.source_start_time
        );

        let mut overlapping_evaluated = AviQtlWaveformEvaluatedPoint {
            start_time: 42.0,
            ..AviQtlWaveformEvaluatedPoint::default()
        };
        let evaluated_pointer = std::ptr::addr_of!(overlapping_evaluated);
        let overlapping_plan = std::ptr::addr_of_mut!(overlapping_evaluated).cast();
        // SAFETY: The output deliberately aliases the aligned evaluated-point allocation and
        // must be rejected before either range is borrowed or modified.
        let status = unsafe {
            aviqtl_audio_plan_waveform(
                &AviQtlWaveformContext {
                    pixel_width: 1,
                    ..waveform_context
                },
                evaluated_pointer,
                1,
                overlapping_plan,
                1,
            )
        };
        assert_eq!(status, STATUS_OVERLAPPING_BUFFERS);
        assert_eq!(overlapping_evaluated.start_time, 42.0);
    }
}
