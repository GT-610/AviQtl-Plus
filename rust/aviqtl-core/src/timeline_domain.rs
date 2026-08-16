use crate::abi::{
    AviQtlIdAllocation, AviQtlSceneSettings, AviQtlTimelineClipGeometry, STATUS_BUFFER_TOO_SMALL,
    STATUS_INVALID_ARGUMENT, STATUS_OK, STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use std::collections::BTreeSet;

const DEFAULT_WIDTH: i32 = 1920;
const DEFAULT_HEIGHT: i32 = 1080;
const DEFAULT_FPS: f64 = 60.0;
const DEFAULT_TOTAL_FRAMES: i32 = 300;
const MAX_DIMENSION: i32 = 32_768;
const MAX_FPS: f64 = 1_000.0;
const MAX_GRID_BPM: f64 = 1_000.0;
const MAX_GRID_OFFSET: f64 = 86_400.0;
const MAX_GRID_INTERVAL: i32 = 1_000_000;
const MAX_GRID_SUBDIVISION: i32 = 128;
const MAX_MAGNETIC_SNAP_RANGE: i32 = 100;
const GRID_MODE_COUNT: u32 = 3;

fn bounded_positive(value: i32, maximum: i32, fallback: i32) -> i32 {
    if value <= 0 || value > maximum {
        fallback
    } else {
        value
    }
}

fn bounded_positive_f64(value: f64, maximum: f64, fallback: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 || value > maximum {
        fallback
    } else {
        value
    }
}

pub(crate) fn allocate_id(
    existing_ids: &[i32],
    next_hint: i32,
    minimum_id: i32,
) -> Option<AviQtlIdAllocation> {
    let minimum_id = minimum_id.max(0);
    let mut candidate = next_hint.max(minimum_id);
    while existing_ids.contains(&candidate) {
        candidate = candidate.checked_add(1)?;
    }
    Some(AviQtlIdAllocation {
        allocated_id: candidate,
        next_id: candidate.saturating_add(1),
    })
}

fn normalize_scene_settings(input: AviQtlSceneSettings) -> AviQtlSceneSettings {
    let grid_offset =
        if input.grid_offset.is_finite() && (0.0..=MAX_GRID_OFFSET).contains(&input.grid_offset) {
            input.grid_offset
        } else {
            0.0
        };
    AviQtlSceneSettings {
        width: bounded_positive(input.width, MAX_DIMENSION, DEFAULT_WIDTH),
        height: bounded_positive(input.height, MAX_DIMENSION, DEFAULT_HEIGHT),
        fps: bounded_positive_f64(input.fps, MAX_FPS, DEFAULT_FPS),
        total_frames: bounded_positive(input.total_frames, i32::MAX, DEFAULT_TOTAL_FRAMES),
        grid_mode: if input.grid_mode < GRID_MODE_COUNT {
            input.grid_mode
        } else {
            0
        },
        grid_bpm: bounded_positive_f64(input.grid_bpm, MAX_GRID_BPM, 120.0),
        grid_offset,
        grid_interval: bounded_positive(input.grid_interval, MAX_GRID_INTERVAL, 10),
        grid_subdivision: bounded_positive(input.grid_subdivision, MAX_GRID_SUBDIVISION, 4),
        enable_snap: u32::from(input.enable_snap != 0),
        magnetic_snap_range: bounded_positive(
            input.magnetic_snap_range,
            MAX_MAGNETIC_SNAP_RANGE,
            10,
        ),
    }
}

fn rounded_non_negative(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(0.0, f64::from(i32::MAX)) as i32
}

fn snap_frame(
    frame: f64,
    ignore_snap: bool,
    settings: AviQtlSceneSettings,
    timeline_scale: f64,
) -> i32 {
    if ignore_snap || settings.enable_snap == 0 {
        return rounded_non_negative(frame);
    }

    let fps = if settings.fps.is_finite() {
        settings.fps.max(1.0)
    } else {
        DEFAULT_FPS
    };
    let scale = if timeline_scale.is_finite() {
        timeline_scale
    } else {
        1.0
    };
    let mut step = 1.0;
    let mut offset = 0.0;
    match settings.grid_mode {
        1 => {
            let bpm = if settings.grid_bpm.is_finite() {
                settings.grid_bpm.max(1.0)
            } else {
                120.0
            };
            let subdivision = if scale > 3.0 {
                4.0
            } else if scale > 1.5 {
                2.0
            } else {
                1.0
            };
            step = (fps / (bpm / 60.0)) / subdivision;
            offset = if settings.grid_offset.is_finite() {
                settings.grid_offset * fps
            } else {
                0.0
            };
        }
        2 => step = f64::from(settings.grid_interval.max(1)),
        _ if scale < 0.5 => step = fps.ceil(),
        _ if scale < 1.5 => step = 10.0,
        _ if scale < 3.0 => step = 5.0,
        _ => {}
    }
    rounded_non_negative(((frame - offset) / step).round() * step + offset)
}

fn timeline_duration(clips: &[AviQtlTimelineClipGeometry]) -> i32 {
    clips
        .iter()
        .filter(|clip| clip.duration_frames > 0)
        .map(|clip| clip.start_frame.saturating_add(clip.duration_frames))
        .max()
        .unwrap_or(0)
        .max(1)
}

fn clamp_scene_duration(
    requested_duration: i32,
    scene_duration: i32,
    speed: f64,
    offset: i32,
) -> i32 {
    if scene_duration <= 0 || !speed.is_finite() || speed <= 0.0 {
        return requested_duration;
    }
    let remaining = f64::from(scene_duration.saturating_sub(1).saturating_sub(offset));
    let maximum = ((remaining / speed) as i32).saturating_add(1).max(1);
    requested_duration.min(maximum)
}

fn normalize_selection(ids: &[i32], requested_primary: i32) -> (Vec<i32>, i32) {
    let mut seen = BTreeSet::new();
    let normalized: Vec<_> = ids
        .iter()
        .copied()
        .filter(|id| *id >= 0 && seen.insert(*id))
        .collect();
    let primary = if normalized.contains(&requested_primary) {
        requested_primary
    } else {
        normalized.first().copied().unwrap_or(-1)
    };
    (normalized, primary)
}

fn toggle_selection(current_ids: &[i32], current_primary: i32, toggled_id: i32) -> (Vec<i32>, i32) {
    let (mut normalized, mut primary) = normalize_selection(current_ids, current_primary);
    if toggled_id < 0 {
        return (Vec::new(), -1);
    }
    if let Some(index) = normalized.iter().position(|id| *id == toggled_id) {
        normalized.remove(index);
        if primary == toggled_id {
            primary = normalized.first().copied().unwrap_or(-1);
        }
    } else {
        normalized.push(toggled_id);
        primary = toggled_id;
    }
    (normalized, primary)
}

fn normalize_removal_indices(length: usize, indices: &[i32], minimum_index: i32) -> Vec<i32> {
    let minimum_index = minimum_index.max(0);
    let mut normalized = BTreeSet::new();
    for index in indices {
        if *index >= minimum_index && usize::try_from(*index).is_ok_and(|index| index < length) {
            normalized.insert(*index);
        }
    }
    normalized.into_iter().rev().collect()
}

fn inverse_permutation(permutation: &[i32]) -> Option<Vec<i32>> {
    let mut inverse = vec![-1_i32; permutation.len()];
    for (new_index, old_index) in permutation.iter().copied().enumerate() {
        let slot = usize::try_from(old_index)
            .ok()
            .and_then(|index| inverse.get_mut(index))?;
        if *slot >= 0 {
            return None;
        }
        *slot = i32::try_from(new_index).ok()?;
    }
    inverse.iter().all(|index| *index >= 0).then_some(inverse)
}

fn plan_index_move(
    length: usize,
    old_index: i32,
    new_index: i32,
    minimum_index: i32,
) -> Option<(Vec<i32>, Vec<i32>)> {
    if length > i32::MAX as usize
        || old_index < minimum_index
        || new_index < minimum_index
        || usize::try_from(old_index).ok()? >= length
        || usize::try_from(new_index).ok()? >= length
    {
        return None;
    }
    let mut permutation: Vec<_> = (0..length as i32).collect();
    let moved = permutation.remove(old_index as usize);
    permutation.insert(new_index as usize, moved);
    let inverse = inverse_permutation(&permutation)?;
    Some((permutation, inverse))
}

fn plan_multi_reorder(
    length: usize,
    indices: &[i32],
    target_index: i32,
    minimum_index: i32,
) -> Option<(Vec<i32>, Vec<i32>, usize)> {
    if length > i32::MAX as usize || target_index < 0 || minimum_index < 0 {
        return None;
    }
    let selected_ascending: Vec<_> = normalize_removal_indices(length, indices, minimum_index)
        .into_iter()
        .rev()
        .collect();
    if selected_ascending.is_empty() {
        return None;
    }
    let selected: BTreeSet<_> = selected_ascending.iter().copied().collect();
    let remaining: Vec<_> = (0..length as i32)
        .filter(|index| !selected.contains(index))
        .collect();
    let count_before = selected_ascending
        .iter()
        .filter(|index| **index < target_index)
        .count() as i32;
    let insert_at =
        (target_index - count_before).clamp(minimum_index, remaining.len() as i32) as usize;
    let mut permutation = Vec::with_capacity(length);
    permutation.extend_from_slice(&remaining[..insert_at]);
    permutation.extend_from_slice(&selected_ascending);
    permutation.extend_from_slice(&remaining[insert_at..]);
    let inverse = inverse_permutation(&permutation)?;
    Some((permutation, inverse, selected_ascending.len()))
}

unsafe fn write_i32_plan(
    input: *const i32,
    input_length: usize,
    planned: &[i32],
    output: *mut i32,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(input, input_length)
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        ranges_overlap(input, input_length, output, output_capacity),
        ranges_overlap(input, input_length, output_length, 1),
        ranges_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    // SAFETY: The output-length pointer was validated and checked for overlap.
    unsafe { output_length.write(planned.len()) };
    if output_capacity < planned.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !planned.is_empty() {
        // SAFETY: Capacity was checked, and the output is valid and disjoint.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..planned.len()].copy_from_slice(planned);
    }
    STATUS_OK
}

struct SelectionOutput {
    ids: *mut i32,
    capacity: usize,
    length: *mut usize,
    primary: *mut i32,
}

unsafe fn write_selection(
    input: *const i32,
    input_length: usize,
    planned: &[i32],
    primary: i32,
    output: SelectionOutput,
) -> u32 {
    if !slice_is_valid(input, input_length)
        || !slice_is_valid(output.ids, output.capacity)
        || !slice_is_valid(output.length, 1)
        || !slice_is_valid(output.primary, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        ranges_overlap(input, input_length, output.ids, output.capacity),
        ranges_overlap(input, input_length, output.length, 1),
        ranges_overlap(input, input_length, output.primary, 1),
        ranges_overlap(output.ids, output.capacity, output.length, 1),
        ranges_overlap(output.ids, output.capacity, output.primary, 1),
        ranges_overlap(output.length, 1, output.primary, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    // SAFETY: The output-length pointer was validated and checked for overlap.
    unsafe { output.length.write(planned.len()) };
    if output.capacity < planned.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !planned.is_empty() {
        // SAFETY: Capacity was checked, and the output is valid and disjoint.
        let ids = unsafe { std::slice::from_raw_parts_mut(output.ids, output.capacity) };
        ids[..planned.len()].copy_from_slice(planned);
    }
    // SAFETY: The primary output was validated and checked against every other range.
    unsafe { output.primary.write(primary) };
    STATUS_OK
}

/// Allocates the first unused ID at or after the requested hint.
///
/// # Safety
///
/// The input must be readable and the output writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_allocate_id(
    existing_ids: *const i32,
    existing_ids_length: usize,
    next_hint: i32,
    minimum_id: i32,
    output: *mut AviQtlIdAllocation,
) -> u32 {
    if !slice_is_valid(existing_ids, existing_ids_length) || !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlap) = ranges_overlap(existing_ids, existing_ids_length, output, 1) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlap {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let existing_ids = if existing_ids_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(existing_ids, existing_ids_length) }
    };
    let Some(allocation) = allocate_id(existing_ids, next_hint, minimum_id) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The output was validated and checked against the input.
    unsafe { output.write(allocation) };
    STATUS_OK
}

/// Normalizes live scene settings using the same domain limits as project documents.
///
/// # Safety
///
/// Input and output must be valid and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_normalize_scene_settings(
    input: *const AviQtlSceneSettings,
    output: *mut AviQtlSceneSettings,
) -> u32 {
    if !slice_is_valid(input, 1) || !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlap) = ranges_overlap(input, 1, output, 1) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlap {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    // SAFETY: The input and output were validated and checked for overlap.
    let normalized = normalize_scene_settings(unsafe { *input });
    // SAFETY: The output was validated and checked against the input.
    unsafe { output.write(normalized) };
    STATUS_OK
}

/// Snaps a frame using normalized scene-grid semantics.
///
/// # Safety
///
/// The settings pointer must be readable for one element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_snap_frame(
    frame: f64,
    ignore_snap: u32,
    settings: *const AviQtlSceneSettings,
    timeline_scale: f64,
) -> i32 {
    if !slice_is_valid(settings, 1) {
        return rounded_non_negative(frame);
    }
    // SAFETY: The settings pointer was validated above and is only read for this call.
    snap_frame(
        frame,
        ignore_snap != 0,
        unsafe { *settings },
        timeline_scale,
    )
}

/// Computes the visible timeline duration from clip geometry.
///
/// # Safety
///
/// The clip range must be readable for `clips_length` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_duration(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
) -> i32 {
    if !slice_is_valid(clips, clips_length) {
        return 1;
    }
    let clips = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The clip range was validated above and is only borrowed for this call.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    timeline_duration(clips)
}

/// Clamps an output duration to the remaining source-scene frames, where `speed` is source
/// frames consumed per output frame and `offset` is measured in source frames.
#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_timeline_clamp_scene_duration(
    requested_duration: i32,
    scene_duration: i32,
    speed: f64,
    offset: i32,
) -> i32 {
    clamp_scene_duration(requested_duration, scene_duration, speed, offset)
}

/// Replaces a selection while preserving the first occurrence of each non-negative ID.
///
/// # Safety
///
/// All ranges must be valid and mutable outputs must be mutually disjoint from the input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_selection_replace(
    ids: *const i32,
    ids_length: usize,
    requested_primary: i32,
    output: *mut i32,
    output_capacity: usize,
    output_length: *mut usize,
    output_primary: *mut i32,
) -> u32 {
    if !slice_is_valid(ids, ids_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    let ids_slice = if ids_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated. Output overlap is checked before writes.
        unsafe { std::slice::from_raw_parts(ids, ids_length) }
    };
    let (planned, primary) = normalize_selection(ids_slice, requested_primary);
    // SAFETY: The helper validates all output ranges and overlap before writing.
    unsafe {
        write_selection(
            ids,
            ids_length,
            &planned,
            primary,
            SelectionOutput {
                ids: output,
                capacity: output_capacity,
                length: output_length,
                primary: output_primary,
            },
        )
    }
}

/// Toggles one selection ID, promoting the first remaining ID when the primary is removed.
///
/// # Safety
///
/// All ranges must be valid and mutable outputs must be mutually disjoint from the input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_selection_toggle(
    current_ids: *const i32,
    current_ids_length: usize,
    current_primary: i32,
    toggled_id: i32,
    output: *mut i32,
    output_capacity: usize,
    output_length: *mut usize,
    output_primary: *mut i32,
) -> u32 {
    if !slice_is_valid(current_ids, current_ids_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    let current = if current_ids_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated. Output overlap is checked before writes.
        unsafe { std::slice::from_raw_parts(current_ids, current_ids_length) }
    };
    let (planned, primary) = toggle_selection(current, current_primary, toggled_id);
    // SAFETY: The helper validates all output ranges and overlap before writing.
    unsafe {
        write_selection(
            current_ids,
            current_ids_length,
            &planned,
            primary,
            SelectionOutput {
                ids: output,
                capacity: output_capacity,
                length: output_length,
                primary: output_primary,
            },
        )
    }
}

/// Normalizes effect-removal indices into a unique descending plan.
///
/// # Safety
///
/// All ranges must be valid and outputs disjoint from the input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_normalize_removal_indices(
    length: usize,
    indices: *const i32,
    indices_length: usize,
    minimum_index: i32,
    output: *mut i32,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(indices, indices_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    let indices_slice = if indices_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated. Output overlap is checked before writes.
        unsafe { std::slice::from_raw_parts(indices, indices_length) }
    };
    let planned = normalize_removal_indices(length, indices_slice, minimum_index);
    // SAFETY: The helper validates every range and overlap before writing.
    unsafe {
        write_i32_plan(
            indices,
            indices_length,
            &planned,
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Builds redo and undo permutations for one list item move.
///
/// # Safety
///
/// Both output arrays must contain `length` writable elements and be disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_index_move(
    length: usize,
    old_index: i32,
    new_index: i32,
    minimum_index: i32,
    redo: *mut i32,
    undo: *mut i32,
) -> u32 {
    if !slice_is_valid(redo, length) || !slice_is_valid(undo, length) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlap) = ranges_overlap(redo, length, undo, length) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlap {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let Some((planned_redo, planned_undo)) =
        plan_index_move(length, old_index, new_index, minimum_index)
    else {
        return STATUS_INVALID_ARGUMENT;
    };
    if length != 0 {
        // SAFETY: Both outputs were validated and checked against each other.
        unsafe {
            std::slice::from_raw_parts_mut(redo, length).copy_from_slice(&planned_redo);
            std::slice::from_raw_parts_mut(undo, length).copy_from_slice(&planned_undo);
        }
    }
    STATUS_OK
}

/// Builds redo and undo permutations for a stable multi-item reorder.
///
/// # Safety
///
/// The selected input must be readable. Both permutation outputs and the selected-count output
/// must be writable, mutually disjoint, and disjoint from the input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_multi_reorder(
    length: usize,
    indices: *const i32,
    indices_length: usize,
    target_index: i32,
    minimum_index: i32,
    redo: *mut i32,
    undo: *mut i32,
    output_selected_count: *mut usize,
) -> u32 {
    if !slice_is_valid(indices, indices_length)
        || !slice_is_valid(redo, length)
        || !slice_is_valid(undo, length)
        || !slice_is_valid(output_selected_count, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        ranges_overlap(indices, indices_length, redo, length),
        ranges_overlap(indices, indices_length, undo, length),
        ranges_overlap(indices, indices_length, output_selected_count, 1),
        ranges_overlap(redo, length, undo, length),
        ranges_overlap(redo, length, output_selected_count, 1),
        ranges_overlap(undo, length, output_selected_count, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let indices = if indices_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against every output.
        unsafe { std::slice::from_raw_parts(indices, indices_length) }
    };
    let Some((planned_redo, planned_undo, selected_count)) =
        plan_multi_reorder(length, indices, target_index, minimum_index)
    else {
        return STATUS_INVALID_ARGUMENT;
    };
    if length != 0 {
        // SAFETY: Both outputs were validated and checked against every other range.
        unsafe {
            std::slice::from_raw_parts_mut(redo, length).copy_from_slice(&planned_redo);
            std::slice::from_raw_parts_mut(undo, length).copy_from_slice(&planned_undo);
        }
    }
    // SAFETY: The count output was validated and checked against every other range.
    unsafe { output_selected_count.write(selected_count) };
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_skips_existing_ids_and_respects_minimum() {
        assert_eq!(
            allocate_id(&[1, 2, 4], 1, 1),
            Some(AviQtlIdAllocation {
                allocated_id: 3,
                next_id: 4,
            })
        );
        assert_eq!(allocate_id(&[], -5, 1).unwrap().allocated_id, 1);
    }

    #[test]
    fn scene_settings_use_domain_limits() {
        let normalized = normalize_scene_settings(AviQtlSceneSettings {
            width: 0,
            height: 50_000,
            fps: f64::NAN,
            total_frames: -1,
            grid_mode: 99,
            grid_bpm: 5_000.0,
            grid_offset: -1.0,
            grid_interval: 0,
            grid_subdivision: 500,
            enable_snap: 9,
            magnetic_snap_range: 500,
        });
        assert_eq!(normalized.width, DEFAULT_WIDTH);
        assert_eq!(normalized.height, DEFAULT_HEIGHT);
        assert_eq!(normalized.fps, DEFAULT_FPS);
        assert_eq!(normalized.total_frames, DEFAULT_TOTAL_FRAMES);
        assert_eq!(normalized.grid_mode, 0);
        assert_eq!(normalized.grid_bpm, 120.0);
        assert_eq!(normalized.grid_offset, 0.0);
        assert_eq!(normalized.grid_interval, 10);
        assert_eq!(normalized.grid_subdivision, 4);
        assert_eq!(normalized.enable_snap, 1);
        assert_eq!(normalized.magnetic_snap_range, 10);
    }

    #[test]
    fn grid_snapping_and_duration_rules_match_the_timeline() {
        let settings = AviQtlSceneSettings {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            fps: 30.0,
            total_frames: DEFAULT_TOTAL_FRAMES,
            grid_mode: 2,
            grid_bpm: 120.0,
            grid_offset: 0.0,
            grid_interval: 10,
            grid_subdivision: 4,
            enable_snap: 1,
            magnetic_snap_range: 10,
        };
        assert_eq!(snap_frame(16.0, false, settings, 1.0), 20);
        assert_eq!(snap_frame(16.0, true, settings, 1.0), 16);

        let clips = [
            AviQtlTimelineClipGeometry {
                clip_id: 1,
                layer: 0,
                start_frame: 5,
                duration_frames: 10,
            },
            AviQtlTimelineClipGeometry {
                clip_id: 2,
                layer: 1,
                start_frame: 20,
                duration_frames: 30,
            },
        ];
        assert_eq!(timeline_duration(&clips), 50);
        assert_eq!(timeline_duration(&[]), 1);
        assert_eq!(clamp_scene_duration(100, 60, 2.0, 9), 26);
    }

    #[test]
    fn selection_preserves_order_and_primary_rules() {
        assert_eq!(normalize_selection(&[7, 3, 7, -1], 7), (vec![7, 3], 7));
        assert_eq!(normalize_selection(&[7, 3], 9), (vec![7, 3], 7));
        assert_eq!(toggle_selection(&[7, 3], 7, 7), (vec![3], 3));
        assert_eq!(toggle_selection(&[7, 3], 7, 5), (vec![7, 3, 5], 5));
        assert_eq!(toggle_selection(&[7, 3], 7, -1), (Vec::new(), -1));
    }

    #[test]
    fn selection_replace_reports_required_capacity_without_partial_writes() {
        let ids = [7_i32, 3_i32, 7_i32];
        let mut output = [99_i32];
        let original = output;
        let mut required = 0_usize;
        let mut primary = 99_i32;
        // SAFETY: Every range is valid and disjoint; the ID output is intentionally undersized.
        let status = unsafe {
            aviqtl_selection_replace(
                ids.as_ptr(),
                ids.len(),
                7,
                output.as_mut_ptr(),
                output.len(),
                &mut required,
                &mut primary,
            )
        };
        assert_eq!(status, STATUS_BUFFER_TOO_SMALL);
        assert_eq!(required, 2);
        assert_eq!(output, original);
        assert_eq!(primary, 99);
    }

    #[test]
    fn removal_and_reorder_plans_are_stable_and_invertible() {
        assert_eq!(normalize_removal_indices(6, &[3, 1, 3, 0, 8], 1), [3, 1]);

        let (redo, undo) = plan_index_move(5, 1, 4, 1).unwrap();
        assert_eq!(redo, [0, 2, 3, 4, 1]);
        assert_eq!(undo, [0, 4, 1, 2, 3]);

        let (redo, undo, count) = plan_multi_reorder(6, &[4, 2, 4, 0], 5, 1).unwrap();
        assert_eq!(count, 2);
        assert_eq!(redo, [0, 1, 3, 2, 4, 5]);
        let restored: Vec<_> = undo.iter().map(|index| redo[*index as usize]).collect();
        assert_eq!(restored, [0, 1, 2, 3, 4, 5]);
    }
}
