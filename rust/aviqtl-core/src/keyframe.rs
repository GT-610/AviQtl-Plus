use crate::abi::{
    AviQtlEasingParameters, AviQtlNumericKeyframe, AviQtlNumericTrackView, STATUS_INVALID_ARGUMENT,
    STATUS_OK, STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid, utf8,
};
use crate::{EasingKind, evaluate};

const INTERPOLATION_NONE: u32 = 42;
const INTERPOLATION_RANDOM: u32 = 43;
const INTERPOLATION_ALTERNATE: u32 = 44;
const INTERPOLATION_COUNT: u32 = 45;
const CUSTOM_POINT_STRIDE: u32 = 6;

fn interpolation_is_valid(value: u32) -> bool {
    value < INTERPOLATION_COUNT
}

fn interpolation_from_name(name: &str) -> u32 {
    EasingKind::from_name(name).map_or_else(
        || match name {
            "none" => INTERPOLATION_NONE,
            "random" => INTERPOLATION_RANDOM,
            "alternate" => INTERPOLATION_ALTERNATE,
            _ => EasingKind::Linear as u32,
        },
        |kind| kind as u32,
    )
}

/// Resolves a numeric interpolation kind from a UTF-8 name.
///
/// # Safety
///
/// `value` must be valid for `value_length` readable bytes for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_numeric_interpolation_from_name(
    value: *const u8,
    value_length: usize,
) -> u32 {
    // SAFETY: The caller upholds the byte-range contract above.
    unsafe { utf8(value, value_length) }.map_or(EasingKind::Linear as u32, interpolation_from_name)
}

fn checked_custom_points<'a>(
    track: &'a AviQtlNumericTrackView,
    keyframe: &AviQtlNumericKeyframe,
) -> Option<&'a [f64]> {
    let offset = keyframe.custom_points_offset as usize;
    let length = keyframe.custom_points_length as usize;
    let end = offset.checked_add(length)?;
    if end > track.custom_points_length {
        return None;
    }
    if length == 0 {
        return Some(&[]);
    }
    // SAFETY: The track's custom-point range is validated before evaluation,
    // and the checked offset and length remain inside that range.
    Some(unsafe { std::slice::from_raw_parts(track.custom_points.add(offset), length) })
}

#[cfg(target_pointer_width = "64")]
fn qt_hash_word(mut key: usize) -> usize {
    key ^= key >> 32;
    key = key.wrapping_mul(0xd6e8_feb8_6659_fd93);
    key ^= key >> 32;
    key = key.wrapping_mul(0xd6e8_feb8_6659_fd93);
    key ^ (key >> 32)
}

#[cfg(target_pointer_width = "32")]
fn qt_hash_word(mut key: usize) -> usize {
    key ^= key >> 16;
    key = key.wrapping_mul(0x045d_9f3b);
    key ^= key >> 16;
    key = key.wrapping_mul(0x045d_9f3b);
    key ^ (key >> 16)
}

fn qt_hash_i32(value: i32) -> usize {
    qt_hash_word(value as isize as usize)
}

#[cfg(target_pointer_width = "64")]
fn qt_hash_i64(value: i64) -> usize {
    qt_hash_word(value as usize)
}

#[cfg(target_pointer_width = "32")]
fn qt_hash_i64(value: i64) -> usize {
    let bits = value as u64;
    qt_hash_word(((bits as u32) ^ ((bits >> 32) as u32)) as usize)
}

fn scaled_random_value(value: f64) -> Option<i64> {
    let scaled = value * 1000.0;
    if scaled.is_finite() && scaled >= i64::MIN as f64 && scaled < i64::MAX as f64 {
        Some(scaled as i64)
    } else {
        None
    }
}

fn validate_track(track: &AviQtlNumericTrackView, output: *mut f64, output_length: usize) -> bool {
    if !slice_is_valid(track.keyframes, track.keyframes_length)
        || !slice_is_valid(track.custom_points, track.custom_points_length)
    {
        return false;
    }
    let Some(keyframes_overlap_output) = ranges_overlap(
        track.keyframes,
        track.keyframes_length,
        output,
        output_length,
    ) else {
        return false;
    };
    let Some(points_overlap_output) = ranges_overlap(
        track.custom_points,
        track.custom_points_length,
        output,
        output_length,
    ) else {
        return false;
    };
    if keyframes_overlap_output || points_overlap_output {
        return false;
    }
    if track.keyframes_length == 0 {
        return true;
    }

    // SAFETY: The pointer, alignment, byte length, and output non-overlap were
    // checked above. The caller keeps the input range alive for the call.
    let keyframes = unsafe { std::slice::from_raw_parts(track.keyframes, track.keyframes_length) };
    for (index, keyframe) in keyframes.iter().enumerate() {
        if !interpolation_is_valid(keyframe.interpolation)
            || checked_custom_points(track, keyframe).is_none()
            || (keyframe.interpolation == EasingKind::Custom as u32
                && (keyframe.custom_points_length < CUSTOM_POINT_STRIDE
                    || !keyframe
                        .custom_points_length
                        .is_multiple_of(CUSTOM_POINT_STRIDE)))
            || (index != 0 && keyframes[index - 1].frame > keyframe.frame)
            || (index != 0
                && i64::from(keyframe.frame) - i64::from(keyframes[index - 1].frame)
                    > i64::from(i32::MAX))
            || ((keyframe.interpolation == INTERPOLATION_RANDOM
                || keyframe.interpolation == INTERPOLATION_ALTERNATE)
                && keyframe.step_frames > i32::MAX as u32)
        {
            return false;
        }
        if keyframe.interpolation == INTERPOLATION_RANDOM
            && (scaled_random_value(keyframe.value).is_none()
                || (index + 1 < keyframes.len()
                    && scaled_random_value(keyframes[index + 1].value).is_none()))
        {
            return false;
        }
    }
    true
}

fn random_interpolation(a: f64, b: f64, f0: i32, f1: i32, step_index: i32) -> f64 {
    let seed = qt_hash_i32(f0)
        ^ qt_hash_i32(f1)
        ^ qt_hash_i32(step_index)
        ^ qt_hash_i64(scaled_random_value(a).expect("validated random value"))
        ^ qt_hash_i64(scaled_random_value(b).expect("validated random value"));
    let fraction = f64::from((seed as u32) % 1_000_000) / 999_999.0;
    a.min(b) + (a.max(b) - a.min(b)) * fraction
}

pub(crate) struct NumericSegment<'a> {
    pub interpolation: &'a str,
    pub first_value: f64,
    pub second_value: f64,
    pub first_frame: i32,
    pub second_frame: i32,
    pub frame: i32,
    pub custom_points: &'a [f64],
    pub amplitude: f64,
    pub period: f64,
    pub step_frames: i32,
}

pub(crate) fn evaluate_numeric_segment(segment: NumericSegment<'_>) -> f64 {
    if segment.first_frame == segment.second_frame {
        return segment.first_value;
    }
    let frame_offset = i64::from(segment.frame) - i64::from(segment.first_frame);
    let frame_delta = i64::from(segment.second_frame) - i64::from(segment.first_frame);
    let t = frame_offset as f64 / frame_delta as f64;
    match segment.interpolation {
        "none" => segment.first_value,
        "random" => {
            if scaled_random_value(segment.first_value).is_none()
                || scaled_random_value(segment.second_value).is_none()
            {
                return segment.first_value;
            }
            let step_frames = i64::from(segment.step_frames.max(1));
            random_interpolation(
                segment.first_value,
                segment.second_value,
                segment.first_frame,
                segment.second_frame,
                (frame_offset / step_frames) as i32,
            )
        }
        "alternate" => {
            let step_frames = i64::from(segment.step_frames.max(1));
            if (frame_offset / step_frames) % 2 == 0 {
                segment.first_value
            } else {
                segment.second_value
            }
        }
        name => {
            let kind = EasingKind::from_name(name).unwrap_or(EasingKind::Linear);
            let eased = evaluate(
                kind,
                t,
                segment.custom_points,
                AviQtlEasingParameters {
                    amplitude: segment.amplitude,
                    period: segment.period,
                },
            );
            segment.first_value + (segment.second_value - segment.first_value) * eased
        }
    }
}

fn evaluate_track(track: &AviQtlNumericTrackView, frame: i32) -> f64 {
    if track.keyframes_length == 0 {
        return track.fallback_value;
    }
    // SAFETY: Every track is fully validated before any output is written.
    let keyframes = unsafe { std::slice::from_raw_parts(track.keyframes, track.keyframes_length) };
    if frame <= keyframes[0].frame {
        return keyframes[0].value;
    }
    if frame >= keyframes[keyframes.len() - 1].frame {
        return keyframes[keyframes.len() - 1].value;
    }

    let upper = keyframes.partition_point(|keyframe| keyframe.frame < frame);
    if keyframes[upper].frame == frame {
        return keyframes[upper].value;
    }
    let first = &keyframes[upper - 1];
    let second = &keyframes[upper];
    let frame_offset = i64::from(frame) - i64::from(first.frame);
    let frame_delta = i64::from(second.frame) - i64::from(first.frame);
    let t = frame_offset as f64 / frame_delta as f64;

    match first.interpolation {
        INTERPOLATION_NONE => first.value,
        INTERPOLATION_RANDOM => {
            let step_frames = i64::from(first.step_frames.max(1));
            random_interpolation(
                first.value,
                second.value,
                first.frame,
                second.frame,
                (frame_offset / step_frames) as i32,
            )
        }
        INTERPOLATION_ALTERNATE => {
            let step_frames = i64::from(first.step_frames.max(1));
            if (frame_offset / step_frames) % 2 == 0 {
                first.value
            } else {
                second.value
            }
        }
        easing => {
            let kind = EasingKind::from_abi(easing).expect("validated easing kind");
            let points = checked_custom_points(track, first).expect("validated custom-point slice");
            let eased = evaluate(
                kind,
                t,
                points,
                AviQtlEasingParameters {
                    amplitude: first.amplitude,
                    period: first.period,
                },
            );
            first.value + (second.value - first.value) * eased
        }
    }
}

fn evaluate_discrete_track(track: &AviQtlNumericTrackView, frame: i32) -> f64 {
    if track.keyframes_length == 0 {
        return track.fallback_value;
    }
    // SAFETY: Every track is fully validated before evaluation.
    let keyframes = unsafe { std::slice::from_raw_parts(track.keyframes, track.keyframes_length) };
    if frame <= keyframes[0].frame {
        return keyframes[0].value;
    }
    if frame >= keyframes[keyframes.len() - 1].frame {
        return keyframes[keyframes.len() - 1].value;
    }

    let upper = keyframes.partition_point(|keyframe| keyframe.frame < frame);
    if keyframes[upper].frame == frame {
        return keyframes[upper].value;
    }
    let first = &keyframes[upper - 1];
    let second = &keyframes[upper];
    if first.interpolation == INTERPOLATION_NONE {
        return first.value;
    }
    let offset = i64::from(frame) - i64::from(first.frame);
    let length = i64::from(second.frame) - i64::from(first.frame);
    if offset.saturating_mul(2) < length {
        first.value
    } else {
        second.value
    }
}

/// Evaluates one numeric track with optional discrete-value semantics.
///
/// # Safety
///
/// `track` and `output` must be aligned and valid for one element. The output must not overlap
/// the track descriptor or either nested input range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_numeric_keyframe_evaluate_typed(
    track: *const AviQtlNumericTrackView,
    frame: i32,
    discrete: u32,
    output: *mut f64,
) -> u32 {
    if !slice_is_valid(track, 1) || !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlaps) = ranges_overlap(track, 1, output, 1) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlaps {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    // SAFETY: The descriptor range was validated above and is copied before nested validation.
    let view = unsafe { track.read() };
    if !validate_track(&view, output, 1) {
        let nested_overlap = ranges_overlap(view.keyframes, view.keyframes_length, output, 1)
            .unwrap_or(false)
            || ranges_overlap(view.custom_points, view.custom_points_length, output, 1)
                .unwrap_or(false);
        return if nested_overlap {
            STATUS_OVERLAPPING_BUFFERS
        } else {
            STATUS_INVALID_ARGUMENT
        };
    }
    let value = if discrete != 0 {
        evaluate_discrete_track(&view, frame)
    } else {
        evaluate_track(&view, frame)
    };
    // SAFETY: The output pointer was validated and does not overlap any input range.
    unsafe { output.write(value) };
    STATUS_OK
}

/// Evaluates several numeric keyframe tracks into caller-owned output storage.
///
/// # Safety
///
/// Every non-empty top-level or nested range must be aligned, initialized, and
/// valid for the duration of the call. The output range must be writable and
/// must not overlap the track views or any nested keyframe/custom-point range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_numeric_keyframe_batch_evaluate(
    tracks: *const AviQtlNumericTrackView,
    tracks_length: usize,
    frame: i32,
    output: *mut f64,
    output_length: usize,
) -> u32 {
    if tracks_length != output_length
        || !slice_is_valid(tracks, tracks_length)
        || !slice_is_valid(output, output_length)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlaps) = ranges_overlap(tracks, tracks_length, output, output_length) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlaps {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    if tracks_length == 0 {
        return STATUS_OK;
    }

    for index in 0..tracks_length {
        // SAFETY: The top-level range was validated above. Reading a copy does
        // not create references before its nested ranges are validated.
        let track = unsafe { tracks.add(index).read() };
        if !validate_track(&track, output, output_length) {
            let nested_overlap = ranges_overlap(
                track.keyframes,
                track.keyframes_length,
                output,
                output_length,
            )
            .unwrap_or(false)
                || ranges_overlap(
                    track.custom_points,
                    track.custom_points_length,
                    output,
                    output_length,
                )
                .unwrap_or(false);
            return if nested_overlap {
                STATUS_OVERLAPPING_BUFFERS
            } else {
                STATUS_INVALID_ARGUMENT
            };
        }
    }

    for index in 0..tracks_length {
        // SAFETY: All descriptors and nested slices were validated before this
        // loop, and output is aligned, writable, and non-overlapping.
        let track = unsafe { tracks.add(index).read() };
        unsafe { output.add(index).write(evaluate_track(&track, frame)) };
    }
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    fn keyframe(frame: i32, value: f64, interpolation: u32) -> AviQtlNumericKeyframe {
        AviQtlNumericKeyframe {
            frame,
            interpolation,
            step_frames: 1,
            custom_points_offset: 0,
            custom_points_length: 0,
            reserved: 0,
            value,
            amplitude: 1.0,
            period: 0.3,
        }
    }

    fn view(
        keyframes: &[AviQtlNumericKeyframe],
        points: &[f64],
        fallback: f64,
    ) -> AviQtlNumericTrackView {
        AviQtlNumericTrackView {
            keyframes: keyframes.as_ptr(),
            keyframes_length: keyframes.len(),
            custom_points: points.as_ptr(),
            custom_points_length: points.len(),
            fallback_value: fallback,
        }
    }

    #[test]
    fn evaluates_linear_empty_none_random_alternate_and_duplicates() {
        let linear = [keyframe(0, 0.0, 0), keyframe(100, 100.0, 0)];
        let none = [
            keyframe(0, 10.0, INTERPOLATION_NONE),
            keyframe(100, 20.0, 0),
        ];
        let mut random = [
            keyframe(0, 0.0, INTERPOLATION_RANDOM),
            keyframe(100, 10.0, 0),
        ];
        random[0].step_frames = 5;
        let mut alternate = [
            keyframe(0, 10.0, INTERPOLATION_ALTERNATE),
            keyframe(100, 20.0, 0),
        ];
        alternate[0].step_frames = 5;
        let duplicate = [
            keyframe(0, 0.0, 0),
            keyframe(50, 10.0, 0),
            keyframe(50, 20.0, 0),
            keyframe(100, 100.0, 0),
        ];
        let tracks = [
            view(&linear, &[], -1.0),
            view(&[], &[], 7.0),
            view(&none, &[], -1.0),
            view(&random, &[], -1.0),
            view(&alternate, &[], -1.0),
            view(&duplicate, &[], -1.0),
        ];
        let mut output = [0.0; 6];
        // SAFETY: All arrays are valid, aligned, and non-overlapping.
        let status = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                tracks.as_ptr(),
                tracks.len(),
                50,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert_eq!(status, STATUS_OK);
        assert_eq!(output[0], 50.0);
        assert_eq!(output[1], 7.0);
        assert_eq!(output[2], 10.0);
        assert!(output[3] >= 0.0 && output[3] <= 10.0);
        assert_eq!(output[4], 10.0);
        assert_eq!(output[5], 10.0);
    }

    #[test]
    fn evaluates_custom_points() {
        let points = [0.33, 0.0, 0.66, 1.0, 1.0, 1.0];
        let mut keys = [
            keyframe(0, 0.0, EasingKind::Custom as u32),
            keyframe(100, 100.0, 0),
        ];
        keys[0].custom_points_length = points.len() as u32;
        let tracks = [view(&keys, &points, 0.0)];
        let mut output = [0.0];
        // SAFETY: All arrays are valid, aligned, and non-overlapping.
        let status = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                tracks.as_ptr(),
                tracks.len(),
                50,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert_eq!(status, STATUS_OK);
        assert!((output[0] - 50.0).abs() < 1.0);
    }

    #[test]
    fn typed_evaluation_preserves_discrete_plugin_values() {
        let linear = [keyframe(0, 1.0, 0), keyframe(10, 5.0, 0)];
        let held = [keyframe(0, 2.0, INTERPOLATION_NONE), keyframe(10, 8.0, 0)];
        assert_eq!(evaluate_discrete_track(&view(&linear, &[], 0.0), 4), 1.0);
        assert_eq!(evaluate_discrete_track(&view(&linear, &[], 0.0), 5), 5.0);
        assert_eq!(evaluate_discrete_track(&view(&held, &[], 0.0), 9), 2.0);
    }

    #[test]
    fn interpolation_names_are_owned_by_the_rust_core() {
        assert_eq!(interpolation_from_name("ease_out_bounce"), 37);
        assert_eq!(interpolation_from_name("custom"), 41);
        assert_eq!(interpolation_from_name("none"), INTERPOLATION_NONE);
        assert_eq!(interpolation_from_name("random"), INTERPOLATION_RANDOM);
        assert_eq!(
            interpolation_from_name("alternate"),
            INTERPOLATION_ALTERNATE
        );
        assert_eq!(
            interpolation_from_name("unknown"),
            EasingKind::Linear as u32
        );
    }

    #[test]
    fn rejects_invalid_nested_ranges_without_partial_output() {
        let unsorted = [keyframe(10, 1.0, 0), keyframe(5, 2.0, 0)];
        let valid = [keyframe(0, 0.0, 0), keyframe(10, 10.0, 0)];
        let tracks = [view(&valid, &[], 0.0), view(&unsorted, &[], 0.0)];
        let mut output = [91.0, 92.0];
        // SAFETY: The ranges are valid; the unsorted content is semantically invalid.
        let status = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                tracks.as_ptr(),
                tracks.len(),
                5,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert_eq!(status, STATUS_INVALID_ARGUMENT);
        assert_eq!(output, [91.0, 92.0]);

        let short_custom_points = [0.33, 0.0, 0.66, 1.0];
        let mut invalid_custom = [
            keyframe(0, 0.0, EasingKind::Custom as u32),
            keyframe(10, 10.0, 0),
        ];
        invalid_custom[0].custom_points_length = short_custom_points.len() as u32;
        let invalid_custom_track = view(&invalid_custom, &short_custom_points, 0.0);
        let mut custom_output = [17.0];
        // SAFETY: The ranges are valid; the custom-point count is semantically invalid.
        let status = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                &invalid_custom_track,
                1,
                5,
                custom_output.as_mut_ptr(),
                custom_output.len(),
            )
        };
        assert_eq!(status, STATUS_INVALID_ARGUMENT);
        assert_eq!(custom_output, [17.0]);

        let invalid_view = AviQtlNumericTrackView {
            keyframes: std::ptr::null(),
            keyframes_length: 1,
            custom_points: std::ptr::null(),
            custom_points_length: 0,
            fallback_value: 0.0,
        };
        let mut value = 7.0;
        // SAFETY: The deliberately invalid pointer must be rejected before use.
        let status =
            unsafe { aviqtl_numeric_keyframe_batch_evaluate(&invalid_view, 1, 0, &mut value, 1) };
        assert_eq!(status, STATUS_INVALID_ARGUMENT);
        assert_eq!(value, 7.0);
    }

    #[test]
    fn rejects_top_level_and_nested_output_overlap() {
        let keys = [keyframe(0, 0.0, 0), keyframe(10, 10.0, 0)];
        let track = view(&keys, &[], 0.0);
        // SAFETY: The deliberately overlapping top-level ranges must be rejected.
        let top_level = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                &track,
                1,
                5,
                (&track as *const AviQtlNumericTrackView)
                    .cast_mut()
                    .cast::<f64>(),
                1,
            )
        };
        assert_eq!(top_level, STATUS_OVERLAPPING_BUFFERS);

        let mut overlapping_keys = [keyframe(0, 0.0, 0), keyframe(10, 10.0, 0)];
        let overlapping_track = view(&overlapping_keys, &[], 0.0);
        // SAFETY: Output aliases the nested keyframe storage and must be rejected.
        let nested = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                &overlapping_track,
                1,
                5,
                overlapping_keys.as_mut_ptr().cast::<f64>(),
                1,
            )
        };
        assert_eq!(nested, STATUS_OVERLAPPING_BUFFERS);

        let mut custom_output = [0.33, 0.0, 0.66, 1.0, 1.0, 1.0];
        let mut custom_keys = [
            keyframe(0, 0.0, EasingKind::Custom as u32),
            keyframe(10, 10.0, 0),
        ];
        custom_keys[0].custom_points_length = custom_output.len() as u32;
        let custom_track = AviQtlNumericTrackView {
            keyframes: custom_keys.as_ptr(),
            keyframes_length: custom_keys.len(),
            custom_points: custom_output.as_ptr(),
            custom_points_length: custom_output.len(),
            fallback_value: 0.0,
        };
        // SAFETY: Output aliases the nested custom-point storage and must be rejected.
        let nested_custom = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                &custom_track,
                1,
                5,
                custom_output.as_mut_ptr(),
                1,
            )
        };
        assert_eq!(nested_custom, STATUS_OVERLAPPING_BUFFERS);
    }

    #[test]
    fn rejects_misaligned_top_level_and_nested_pointers() {
        let mut output = [0.0];
        let top_level_storage =
            [0_u8; size_of::<AviQtlNumericTrackView>() + align_of::<AviQtlNumericTrackView>()];
        let top_level_offset = (0..align_of::<AviQtlNumericTrackView>())
            .find(|offset| {
                !(top_level_storage.as_ptr() as usize + offset)
                    .is_multiple_of(align_of::<AviQtlNumericTrackView>())
            })
            .expect("a misaligned offset");
        // SAFETY: The deliberately misaligned descriptor pointer must be rejected before use.
        let top_level = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                top_level_storage.as_ptr().add(top_level_offset).cast(),
                1,
                0,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert_eq!(top_level, STATUS_INVALID_ARGUMENT);

        let nested_storage =
            [0_u8; size_of::<AviQtlNumericKeyframe>() + align_of::<AviQtlNumericKeyframe>()];
        let nested_offset = (0..align_of::<AviQtlNumericKeyframe>())
            .find(|offset| {
                !(nested_storage.as_ptr() as usize + offset)
                    .is_multiple_of(align_of::<AviQtlNumericKeyframe>())
            })
            .expect("a misaligned offset");
        let nested_track = AviQtlNumericTrackView {
            // SAFETY: Creating the raw pointer is valid; the ABI must reject it before dereference.
            keyframes: unsafe { nested_storage.as_ptr().add(nested_offset).cast() },
            keyframes_length: 1,
            custom_points: std::ptr::null(),
            custom_points_length: 0,
            fallback_value: 0.0,
        };
        // SAFETY: The deliberately misaligned nested pointer must be rejected before use.
        let nested = unsafe {
            aviqtl_numeric_keyframe_batch_evaluate(
                &nested_track,
                1,
                0,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert_eq!(nested, STATUS_INVALID_ARGUMENT);
    }
}
