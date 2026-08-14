use crate::abi::{
    AviQtlTimelineClipGeometry, AviQtlTimelineMoveInput, AviQtlTimelinePosition,
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_LOCKED_LAYER, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};

const MIN_LAYER: i32 = 0;
const MAX_LAYER: i32 = 127;

fn clamp_i64(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn clip_end(clip: &AviQtlTimelineClipGeometry) -> i64 {
    i64::from(clip.start_frame) + i64::from(clip.duration_frames.max(0))
}

fn overlaps(
    first_start: i64,
    first_duration: i32,
    second_start: i64,
    second_duration: i32,
) -> bool {
    let first_end = first_start + i64::from(first_duration.max(0));
    let second_end = second_start + i64::from(second_duration.max(0));
    first_start < second_end && second_start < first_end
}

fn is_excluded(clip_id: i32, excluded_ids: &[i32]) -> bool {
    excluded_ids.contains(&clip_id)
}

fn is_locked(layer: i32, locked_layers: &[i32]) -> bool {
    locked_layers.contains(&layer)
}

fn has_internal_overlap(clips: &[AviQtlTimelineClipGeometry]) -> bool {
    clips.iter().enumerate().any(|(index, first)| {
        clips[index + 1..].iter().any(|second| {
            first.layer == second.layer
                && overlaps(
                    i64::from(first.start_frame),
                    first.duration_frames,
                    i64::from(second.start_frame),
                    second.duration_frames,
                )
        })
    })
}

fn find_vacant_frame(
    clips: &[AviQtlTimelineClipGeometry],
    excluded_ids: &[i32],
    layer: i32,
    start_frame: i32,
    duration_frames: i32,
) -> i32 {
    let mut layer_clips: Vec<_> = clips
        .iter()
        .filter(|clip| clip.layer == layer && !is_excluded(clip.clip_id, excluded_ids))
        .collect();
    layer_clips.sort_by_key(|clip| clip.start_frame);

    let mut candidate = i64::from(start_frame.max(0));
    for clip in layer_clips {
        if overlaps(
            candidate,
            duration_frames,
            i64::from(clip.start_frame),
            clip.duration_frames,
        ) {
            candidate = clip_end(clip);
        }
    }
    clamp_i64(candidate)
}

fn group_push(clips: &[AviQtlTimelineClipGeometry], moves: &[AviQtlTimelineMoveInput]) -> i32 {
    let excluded_ids: Vec<_> = moves.iter().map(|movement| movement.clip_id).collect();
    let mut maximum_push = 0_i64;

    for _ in 0..=clips.len() {
        let mut current_push = 0_i64;
        for movement in moves {
            let test_start = i64::from(movement.target_start_frame) + maximum_push;
            let safe_start = i64::from(find_vacant_frame(
                clips,
                &excluded_ids,
                movement.target_layer,
                clamp_i64(test_start),
                movement.duration_frames,
            ));
            current_push = current_push.max(safe_start.saturating_sub(test_start));
        }
        if current_push == 0 {
            break;
        }
        maximum_push = maximum_push.saturating_add(current_push);
    }

    clamp_i64(maximum_push.max(0))
}

fn plan_batch_move(
    clips: &[AviQtlTimelineClipGeometry],
    moves: &[AviQtlTimelineMoveInput],
    locked_layers: &[i32],
) -> Result<Vec<AviQtlTimelineClipGeometry>, u32> {
    if moves.iter().enumerate().any(|(index, movement)| {
        moves[index + 1..]
            .iter()
            .any(|other| other.clip_id == movement.clip_id)
            || !clips.iter().any(|clip| clip.clip_id == movement.clip_id)
    }) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    if moves.iter().any(|movement| {
        is_locked(movement.old_layer, locked_layers)
            || is_locked(movement.target_layer, locked_layers)
    }) {
        return Err(STATUS_LOCKED_LAYER);
    }
    if moves
        .iter()
        .any(|movement| !(MIN_LAYER..=MAX_LAYER).contains(&movement.target_layer))
    {
        return Err(STATUS_INVALID_ARGUMENT);
    }

    let push = i64::from(group_push(clips, moves));
    let planned: Vec<_> = moves
        .iter()
        .map(|movement| AviQtlTimelineClipGeometry {
            clip_id: movement.clip_id,
            layer: movement.target_layer,
            start_frame: clamp_i64((i64::from(movement.target_start_frame) + push).max(0)),
            duration_frames: movement.duration_frames.max(1),
        })
        .collect();
    if has_internal_overlap(&planned) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    Ok(planned)
}

fn plan_delta_move(
    clips: &[AviQtlTimelineClipGeometry],
    moving_ids: &[i32],
    locked_layers: &[i32],
    delta_layer: i32,
    delta_frame: i32,
) -> Result<Vec<AviQtlTimelineClipGeometry>, u32> {
    let mut selected = Vec::with_capacity(moving_ids.len());
    for clip_id in moving_ids {
        if selected
            .iter()
            .any(|clip: &&AviQtlTimelineClipGeometry| clip.clip_id == *clip_id)
        {
            continue;
        }
        let Some(clip) = clips.iter().find(|clip| clip.clip_id == *clip_id) else {
            continue;
        };
        selected.push(clip);
    }
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let minimum_layer = selected
        .iter()
        .map(|clip| clip.layer)
        .min()
        .unwrap_or(MIN_LAYER);
    let maximum_layer = selected
        .iter()
        .map(|clip| clip.layer)
        .max()
        .unwrap_or(MAX_LAYER);
    let effective_delta_layer = i64::from(delta_layer).clamp(
        -i64::from(minimum_layer),
        i64::from(MAX_LAYER) - i64::from(maximum_layer),
    );
    let minimum_start = selected
        .iter()
        .map(|clip| clip.start_frame)
        .min()
        .unwrap_or(0);
    let effective_delta_frame = i64::from(delta_frame).max(-i64::from(minimum_start));

    let mut moves = Vec::with_capacity(selected.len());
    for clip in selected {
        moves.push(AviQtlTimelineMoveInput {
            clip_id: clip.clip_id,
            old_layer: clip.layer,
            old_start_frame: clip.start_frame,
            duration_frames: clip.duration_frames,
            target_layer: clamp_i64(i64::from(clip.layer) + effective_delta_layer),
            target_start_frame: clamp_i64(i64::from(clip.start_frame) + effective_delta_frame),
        });
    }
    plan_batch_move(clips, &moves, locked_layers)
}

fn plan_resize(
    clips: &[AviQtlTimelineClipGeometry],
    delta_start_frame: i32,
    delta_duration_frames: i32,
) -> Vec<AviQtlTimelineClipGeometry> {
    let mut planned = clips.to_vec();
    let descending = delta_start_frame > 0 || delta_duration_frames > 0;
    planned.sort_by(|first, second| {
        let order = first
            .start_frame
            .cmp(&second.start_frame)
            .then(first.layer.cmp(&second.layer));
        if descending { order.reverse() } else { order }
    });
    for clip in &mut planned {
        clip.start_frame = clip.start_frame.saturating_add(delta_start_frame).max(0);
        clip.duration_frames = clip
            .duration_frames
            .saturating_add(delta_duration_frames)
            .max(1);
    }
    planned
}

fn plan_insert_layers(
    clips: &[AviQtlTimelineClipGeometry],
    target_layer: i32,
    count: i32,
    above: bool,
) -> Result<Vec<AviQtlTimelineClipGeometry>, u32> {
    if count <= 0 {
        return Ok(Vec::new());
    }
    let mut planned: Vec<_> = clips
        .iter()
        .copied()
        .filter(|clip| {
            if above {
                clip.layer >= target_layer
            } else {
                clip.layer > target_layer
            }
        })
        .collect();
    planned.sort_by_key(|clip| std::cmp::Reverse(clip.layer));
    for clip in &mut planned {
        let Some(layer) = clip.layer.checked_add(count) else {
            return Err(STATUS_INVALID_ARGUMENT);
        };
        if !(MIN_LAYER..=MAX_LAYER).contains(&layer) {
            return Err(STATUS_INVALID_ARGUMENT);
        }
        clip.layer = layer;
    }
    Ok(planned)
}

fn plan_shift_layers(
    clips: &[AviQtlTimelineClipGeometry],
    start_layer: i32,
    end_layer: i32,
    delta: i32,
) -> Result<Vec<AviQtlTimelineClipGeometry>, u32> {
    if delta == 0 || start_layer > end_layer {
        return Ok(Vec::new());
    }
    let mut planned: Vec<_> = clips
        .iter()
        .copied()
        .filter(|clip| (start_layer..=end_layer).contains(&clip.layer))
        .collect();
    if delta > 0 {
        planned.sort_by_key(|clip| std::cmp::Reverse(clip.layer));
    } else {
        planned.sort_by_key(|clip| clip.layer);
    }
    for clip in &mut planned {
        let Some(layer) = clip.layer.checked_add(delta) else {
            return Err(STATUS_INVALID_ARGUMENT);
        };
        if !(MIN_LAYER..=MAX_LAYER).contains(&layer) {
            return Err(STATUS_INVALID_ARGUMENT);
        }
        clip.layer = layer;
    }
    Ok(planned)
}

fn clipboard_duration(clips: &[AviQtlTimelineClipGeometry]) -> i32 {
    let Some(minimum_start) = clips.iter().map(|clip| i64::from(clip.start_frame)).min() else {
        return 0;
    };
    let maximum_end = clips.iter().map(clip_end).max().unwrap_or(minimum_start);
    clamp_i64(maximum_end.saturating_sub(minimum_start).max(0))
}

#[derive(Clone, Copy)]
struct ClipboardLayout {
    minimum_start: i32,
    minimum_layer: i32,
}

fn clipboard_layout(clips: &[AviQtlTimelineClipGeometry]) -> Option<ClipboardLayout> {
    Some(ClipboardLayout {
        minimum_start: clips.iter().map(|clip| clip.start_frame).min()?,
        minimum_layer: clips.iter().map(|clip| clip.layer).min()?,
    })
}

fn clipboard_layer(
    source: &AviQtlTimelineClipGeometry,
    layout: ClipboardLayout,
    layer_offset: i32,
) -> Result<i32, u32> {
    let layer = i64::from(layer_offset) + i64::from(source.layer) - i64::from(layout.minimum_layer);
    if !(i64::from(MIN_LAYER)..=i64::from(MAX_LAYER)).contains(&layer) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    Ok(layer as i32)
}

fn find_vacant_clipboard_frame(
    existing: &[AviQtlTimelineClipGeometry],
    clipboard: &[AviQtlTimelineClipGeometry],
    requested_frame: i32,
    layer_offset: i32,
) -> Result<i32, u32> {
    if clipboard.is_empty() {
        return Ok(requested_frame);
    }
    let layout = clipboard_layout(clipboard).ok_or(STATUS_INVALID_ARGUMENT)?;
    let mut safe_frame = i64::from(requested_frame.max(0));

    loop {
        let mut next_jump = safe_frame.saturating_add(1);
        let mut colliding = false;
        for source in clipboard {
            let relative_start = i64::from(source.start_frame) - i64::from(layout.minimum_start);
            let clip_start = safe_frame + relative_start;
            let clip_layer = clipboard_layer(source, layout, layer_offset)?;
            for obstacle in existing {
                if obstacle.layer == clip_layer
                    && overlaps(
                        clip_start,
                        source.duration_frames,
                        i64::from(obstacle.start_frame),
                        obstacle.duration_frames,
                    )
                {
                    colliding = true;
                    next_jump = next_jump.max(clip_end(obstacle).saturating_sub(relative_start));
                }
            }
        }
        if !colliding || safe_frame >= i64::from(i32::MAX) {
            return Ok(clamp_i64(safe_frame));
        }
        safe_frame = next_jump.min(i64::from(i32::MAX));
    }
}

fn plan_clipboard_placement(
    existing: &[AviQtlTimelineClipGeometry],
    clipboard: &[AviQtlTimelineClipGeometry],
    requested_frame: i32,
    layer_offset: i32,
) -> Result<(i32, Vec<AviQtlTimelineClipGeometry>), u32> {
    if has_internal_overlap(clipboard) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    if clipboard.is_empty() {
        return Ok((requested_frame.max(0), Vec::new()));
    }
    let layout = clipboard_layout(clipboard).ok_or(STATUS_INVALID_ARGUMENT)?;
    let safe_frame =
        find_vacant_clipboard_frame(existing, clipboard, requested_frame, layer_offset)?;
    let planned = clipboard
        .iter()
        .map(|source| {
            Ok(AviQtlTimelineClipGeometry {
                clip_id: source.clip_id,
                layer: clipboard_layer(source, layout, layer_offset)?,
                start_frame: clamp_i64(
                    i64::from(safe_frame) + i64::from(source.start_frame)
                        - i64::from(layout.minimum_start),
                ),
                duration_frames: source.duration_frames.max(1),
            })
        })
        .collect::<Result<Vec<_>, u32>>()?;
    Ok((safe_frame, planned))
}

fn slices_overlap<T, U>(
    input: *const T,
    input_length: usize,
    output: *const U,
    output_length: usize,
) -> Result<bool, u32> {
    ranges_overlap(input, input_length, output, output_length).ok_or(STATUS_INVALID_ARGUMENT)
}

/// Finds the first non-overlapping frame on one layer.
///
/// # Safety
///
/// Every non-empty range must be aligned, initialized, and valid for the call. The output
/// must be writable and disjoint from both input ranges.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_find_vacant_frame(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    excluded_ids: *const i32,
    excluded_ids_length: usize,
    layer: i32,
    start_frame: i32,
    duration_frames: i32,
    output_frame: *mut i32,
) -> u32 {
    if !slice_is_valid(clips, clips_length)
        || !slice_is_valid(excluded_ids, excluded_ids_length)
        || !slice_is_valid(output_frame, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        slices_overlap(clips, clips_length, output_frame, 1),
        slices_overlap(excluded_ids, excluded_ids_length, output_frame, 1),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let clips = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    let excluded_ids = if excluded_ids_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(excluded_ids, excluded_ids_length) }
    };
    let result = find_vacant_frame(clips, excluded_ids, layer, start_frame, duration_frames);
    // SAFETY: The output was validated and checked against both inputs.
    unsafe { output_frame.write(result) };
    STATUS_OK
}

/// Plans an absolute group move while preserving relative layout.
///
/// # Safety
///
/// All ranges must be valid for the call. The output range must be writable, have one
/// element per movement, and be disjoint from every input range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_batch_move(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    moves: *const AviQtlTimelineMoveInput,
    moves_length: usize,
    locked_layers: *const i32,
    locked_layers_length: usize,
    output: *mut AviQtlTimelineClipGeometry,
    output_length: usize,
) -> u32 {
    if moves_length != output_length
        || !slice_is_valid(clips, clips_length)
        || !slice_is_valid(moves, moves_length)
        || !slice_is_valid(locked_layers, locked_layers_length)
        || !slice_is_valid(output, output_length)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        slices_overlap(clips, clips_length, output, output_length),
        slices_overlap(moves, moves_length, output, output_length),
        slices_overlap(locked_layers, locked_layers_length, output, output_length),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let clips = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    let moves = if moves_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(moves, moves_length) }
    };
    let locked_layers = if locked_layers_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(locked_layers, locked_layers_length) }
    };
    let planned = match plan_batch_move(clips, moves, locked_layers) {
        Ok(planned) => planned,
        Err(status) => return status,
    };
    let output = if output_length == 0 {
        &mut []
    } else {
        // SAFETY: The range was validated and checked against every input.
        unsafe { std::slice::from_raw_parts_mut(output, output_length) }
    };
    output.copy_from_slice(&planned);
    STATUS_OK
}

/// Plans a group move from a shared layer/frame delta.
///
/// # Safety
///
/// All ranges must be valid for the call. Mutable ranges must be disjoint from every input
/// and from each other. The output length receives the required number of elements even when
/// the provided capacity is too small; the output buffer is otherwise left unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_delta_move(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    moving_ids: *const i32,
    moving_ids_length: usize,
    locked_layers: *const i32,
    locked_layers_length: usize,
    delta_layer: i32,
    delta_frame: i32,
    output: *mut AviQtlTimelineClipGeometry,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(clips, clips_length)
        || !slice_is_valid(moving_ids, moving_ids_length)
        || !slice_is_valid(locked_layers, locked_layers_length)
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        slices_overlap(clips, clips_length, output, output_capacity),
        slices_overlap(moving_ids, moving_ids_length, output, output_capacity),
        slices_overlap(locked_layers, locked_layers_length, output, output_capacity),
        slices_overlap(clips, clips_length, output_length, 1),
        slices_overlap(moving_ids, moving_ids_length, output_length, 1),
        slices_overlap(locked_layers, locked_layers_length, output_length, 1),
        slices_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let clips = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against both outputs.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    let moving_ids = if moving_ids_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against both outputs.
        unsafe { std::slice::from_raw_parts(moving_ids, moving_ids_length) }
    };
    let locked_layers = if locked_layers_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against both outputs.
        unsafe { std::slice::from_raw_parts(locked_layers, locked_layers_length) }
    };
    let planned = match plan_delta_move(clips, moving_ids, locked_layers, delta_layer, delta_frame)
    {
        Ok(planned) => planned,
        Err(status) => return status,
    };
    // SAFETY: The output length was validated and checked against every other range.
    unsafe { output_length.write(planned.len()) };
    if output_capacity < planned.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !planned.is_empty() {
        // SAFETY: Capacity was checked, and the output is valid and disjoint.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..planned.len()].copy_from_slice(&planned);
    }
    STATUS_OK
}

/// Resolves a drag operation for one or more clips.
///
/// # Safety
///
/// Input ranges must remain valid for the call. The output must be writable and disjoint
/// from all inputs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_resolve_drag(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    moving_ids: *const i32,
    moving_ids_length: usize,
    locked_layers: *const i32,
    locked_layers_length: usize,
    primary_clip_id: i32,
    target_layer: i32,
    proposed_start_frame: i32,
    output: *mut AviQtlTimelinePosition,
) -> u32 {
    if !slice_is_valid(clips, clips_length)
        || !slice_is_valid(moving_ids, moving_ids_length)
        || !slice_is_valid(locked_layers, locked_layers_length)
        || !slice_is_valid(output, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        slices_overlap(clips, clips_length, output, 1),
        slices_overlap(moving_ids, moving_ids_length, output, 1),
        slices_overlap(locked_layers, locked_layers_length, output, 1),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let clips = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    let moving_ids = if moving_ids_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(moving_ids, moving_ids_length) }
    };
    let locked_layers = if locked_layers_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(locked_layers, locked_layers_length) }
    };
    let Some(primary) = clips.iter().find(|clip| clip.clip_id == primary_clip_id) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let mut resolved_ids = moving_ids.to_vec();
    if !resolved_ids.contains(&primary_clip_id) {
        resolved_ids.push(primary_clip_id);
    }
    let planned = match plan_delta_move(
        clips,
        &resolved_ids,
        locked_layers,
        clamp_i64(i64::from(target_layer) - i64::from(primary.layer)),
        clamp_i64(i64::from(proposed_start_frame) - i64::from(primary.start_frame)),
    ) {
        Ok(planned) => planned,
        Err(status) => return status,
    };
    let Some(primary_result) = planned.iter().find(|clip| clip.clip_id == primary_clip_id) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The output was validated and checked against every input.
    unsafe {
        output.write(AviQtlTimelinePosition {
            frame: primary_result.start_frame,
            layer: primary_result.layer.clamp(MIN_LAYER, MAX_LAYER),
        })
    };
    STATUS_OK
}

/// Plans ordered resize operations for a selected clip group.
///
/// # Safety
///
/// The output must contain one writable element per input clip and be disjoint from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_resize(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    delta_start_frame: i32,
    delta_duration_frames: i32,
    output: *mut AviQtlTimelineClipGeometry,
    output_length: usize,
) -> u32 {
    if clips_length != output_length
        || !slice_is_valid(clips, clips_length)
        || !slice_is_valid(output, output_length)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlap) = ranges_overlap(clips, clips_length, output, output_length) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlap {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let clips = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    let planned = plan_resize(clips, delta_start_frame, delta_duration_frames);
    let output = if output_length == 0 {
        &mut []
    } else {
        // SAFETY: The range was validated and checked against the input.
        unsafe { std::slice::from_raw_parts_mut(output, output_length) }
    };
    output.copy_from_slice(&planned);
    STATUS_OK
}

unsafe fn write_variable_plan(
    input: *const AviQtlTimelineClipGeometry,
    input_length: usize,
    planned: &[AviQtlTimelineClipGeometry],
    output: *mut AviQtlTimelineClipGeometry,
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
        slices_overlap(input, input_length, output, output_capacity),
        slices_overlap(input, input_length, output_length, 1),
        slices_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    // SAFETY: The output length was validated and checked against all other ranges.
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

/// Plans the moves required to insert layers.
///
/// # Safety
///
/// All ranges must be valid and mutable ranges must be disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_insert_layers(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    target_layer: i32,
    count: i32,
    above: u32,
    output: *mut AviQtlTimelineClipGeometry,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(clips, clips_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    let clips_slice = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The input range was validated. Output overlap is checked before writes.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    let planned = match plan_insert_layers(clips_slice, target_layer, count, above != 0) {
        Ok(planned) => planned,
        Err(status) => return status,
    };
    // SAFETY: The helper validates every range before writing.
    unsafe {
        write_variable_plan(
            clips,
            clips_length,
            &planned,
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Plans the moves required to shift a layer range.
///
/// # Safety
///
/// All ranges must be valid and mutable ranges must be disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_shift_layers(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    start_layer: i32,
    end_layer: i32,
    delta: i32,
    output: *mut AviQtlTimelineClipGeometry,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(clips, clips_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    let clips_slice = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The input range was validated. Output overlap is checked before writes.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    let planned = match plan_shift_layers(clips_slice, start_layer, end_layer, delta) {
        Ok(planned) => planned,
        Err(status) => return status,
    };
    // SAFETY: The helper validates every range before writing.
    unsafe {
        write_variable_plan(
            clips,
            clips_length,
            &planned,
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Calculates the total clipboard span.
///
/// # Safety
///
/// The input must be readable and the output writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_clipboard_duration(
    clips: *const AviQtlTimelineClipGeometry,
    clips_length: usize,
    output_duration: *mut i32,
) -> u32 {
    if !slice_is_valid(clips, clips_length) || !slice_is_valid(output_duration, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(overlap) = ranges_overlap(clips, clips_length, output_duration, 1) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if overlap {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let clips = if clips_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(clips, clips_length) }
    };
    // SAFETY: The output was validated and checked against the input.
    unsafe { output_duration.write(clipboard_duration(clips)) };
    STATUS_OK
}

/// Finds a collision-free frame for a multi-clip clipboard layout.
///
/// # Safety
///
/// Both inputs must be readable and the output writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_find_vacant_clipboard_frame(
    existing: *const AviQtlTimelineClipGeometry,
    existing_length: usize,
    clipboard: *const AviQtlTimelineClipGeometry,
    clipboard_length: usize,
    requested_frame: i32,
    layer_offset: i32,
    output_frame: *mut i32,
) -> u32 {
    if !slice_is_valid(existing, existing_length)
        || !slice_is_valid(clipboard, clipboard_length)
        || !slice_is_valid(output_frame, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        slices_overlap(existing, existing_length, output_frame, 1),
        slices_overlap(clipboard, clipboard_length, output_frame, 1),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let existing = if existing_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(existing, existing_length) }
    };
    let clipboard = if clipboard_length == 0 {
        &[]
    } else {
        // SAFETY: The range was validated and checked against the output.
        unsafe { std::slice::from_raw_parts(clipboard, clipboard_length) }
    };
    let result =
        match find_vacant_clipboard_frame(existing, clipboard, requested_frame, layer_offset) {
            Ok(result) => result,
            Err(status) => return status,
        };
    // SAFETY: The output was validated and checked against both inputs.
    unsafe { output_frame.write(result) };
    STATUS_OK
}

/// Plans the final geometry for a clipboard paste operation.
///
/// # Safety
///
/// Both inputs must be readable. The geometry output and frame output must be writable,
/// mutually disjoint, and disjoint from both inputs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_plan_clipboard_placement(
    existing: *const AviQtlTimelineClipGeometry,
    existing_length: usize,
    clipboard: *const AviQtlTimelineClipGeometry,
    clipboard_length: usize,
    requested_frame: i32,
    layer_offset: i32,
    output: *mut AviQtlTimelineClipGeometry,
    output_length: usize,
    output_frame: *mut i32,
) -> u32 {
    if clipboard_length != output_length
        || !slice_is_valid(existing, existing_length)
        || !slice_is_valid(clipboard, clipboard_length)
        || !slice_is_valid(output, output_length)
        || !slice_is_valid(output_frame, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        slices_overlap(existing, existing_length, output, output_length),
        slices_overlap(clipboard, clipboard_length, output, output_length),
        slices_overlap(existing, existing_length, output_frame, 1),
        slices_overlap(clipboard, clipboard_length, output_frame, 1),
        slices_overlap(output, output_length, output_frame, 1),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let existing = if existing_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against both outputs.
        unsafe { std::slice::from_raw_parts(existing, existing_length) }
    };
    let clipboard = if clipboard_length == 0 {
        &[]
    } else {
        // SAFETY: The input was validated and checked against both outputs.
        unsafe { std::slice::from_raw_parts(clipboard, clipboard_length) }
    };
    let (safe_frame, planned) =
        match plan_clipboard_placement(existing, clipboard, requested_frame, layer_offset) {
            Ok(planned) => planned,
            Err(status) => return status,
        };
    let output = if output_length == 0 {
        &mut []
    } else {
        // SAFETY: The output was validated and checked against both inputs and output_frame.
        unsafe { std::slice::from_raw_parts_mut(output, output_length) }
    };
    output.copy_from_slice(&planned);
    // SAFETY: The output was validated and checked against both inputs and geometry output.
    unsafe { output_frame.write(safe_frame) };
    STATUS_OK
}

/// Splits one clip geometry at an interior frame.
///
/// # Safety
///
/// The input must be readable and both outputs writable and mutually disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_split_clip(
    clip: *const AviQtlTimelineClipGeometry,
    frame: i32,
    first: *mut AviQtlTimelineClipGeometry,
    second: *mut AviQtlTimelineClipGeometry,
) -> u32 {
    if !slice_is_valid(clip, 1) || !slice_is_valid(first, 1) || !slice_is_valid(second, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        slices_overlap(clip, 1, first, 1),
        slices_overlap(clip, 1, second, 1),
        slices_overlap(first, 1, second, 1),
    ];
    if overlaps.iter().any(|result| result.is_err()) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    // SAFETY: The input was validated and checked against both outputs.
    let clip = unsafe { *clip };
    let end = i64::from(clip.start_frame) + i64::from(clip.duration_frames);
    if i64::from(frame) <= i64::from(clip.start_frame) || i64::from(frame) >= end {
        return STATUS_INVALID_ARGUMENT;
    }
    let first_duration = frame.saturating_sub(clip.start_frame);
    let second_duration = clip.duration_frames.saturating_sub(first_duration);
    let first_value = AviQtlTimelineClipGeometry {
        duration_frames: first_duration,
        ..clip
    };
    let second_value = AviQtlTimelineClipGeometry {
        start_frame: frame,
        duration_frames: second_duration,
        ..clip
    };
    // SAFETY: Both outputs were validated and checked against each other and the input.
    unsafe {
        first.write(first_value);
        second.write(second_value);
    }
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: i32, layer: i32, start: i32, duration: i32) -> AviQtlTimelineClipGeometry {
        AviQtlTimelineClipGeometry {
            clip_id: id,
            layer,
            start_frame: start,
            duration_frames: duration,
        }
    }

    #[test]
    fn vacant_frame_and_group_move_preserve_layout() {
        let clips = [clip(1, 0, 0, 10), clip(2, 1, 20, 10), clip(3, 0, 15, 10)];
        assert_eq!(find_vacant_frame(&clips, &[1, 2], 0, 10, 10), 25);

        let moves = [
            AviQtlTimelineMoveInput {
                clip_id: 1,
                old_layer: 0,
                old_start_frame: 0,
                duration_frames: 10,
                target_layer: 0,
                target_start_frame: 10,
            },
            AviQtlTimelineMoveInput {
                clip_id: 2,
                old_layer: 1,
                old_start_frame: 20,
                duration_frames: 10,
                target_layer: 1,
                target_start_frame: 30,
            },
        ];
        let planned = plan_batch_move(&clips, &moves, &[]).unwrap();
        assert_eq!(planned[0].start_frame, 25);
        assert_eq!(planned[1].start_frame, 45);
    }

    #[test]
    fn delta_move_preserves_same_layer_layout() {
        let clips = [clip(1, 0, 0, 10), clip(2, 0, 10, 10)];
        let planned = plan_delta_move(&clips, &[1, 2], &[], 0, 5).unwrap();
        assert_eq!(planned, [clip(1, 0, 5, 10), clip(2, 0, 15, 10)]);

        let bounded = [clip(1, 1, 5, 10), clip(2, 3, 20, 10)];
        let planned = plan_delta_move(&bounded, &[1, 2], &[], -4, -20).unwrap();
        assert_eq!(planned, [clip(1, 0, 0, 10), clip(2, 2, 15, 10)]);

        let overlapping = [
            AviQtlTimelineMoveInput {
                clip_id: 1,
                old_layer: 0,
                old_start_frame: 0,
                duration_frames: 10,
                target_layer: 0,
                target_start_frame: 5,
            },
            AviQtlTimelineMoveInput {
                clip_id: 2,
                old_layer: 0,
                old_start_frame: 10,
                duration_frames: 10,
                target_layer: 0,
                target_start_frame: 6,
            },
        ];
        assert_eq!(
            plan_batch_move(&clips, &overlapping, &[]),
            Err(STATUS_INVALID_ARGUMENT)
        );
    }

    #[test]
    fn locked_layers_reject_group_without_output() {
        let clips = [clip(1, 0, 0, 10)];
        let moves = [AviQtlTimelineMoveInput {
            clip_id: 1,
            old_layer: 0,
            old_start_frame: 0,
            duration_frames: 10,
            target_layer: 2,
            target_start_frame: 10,
        }];
        let mut output = [clip(99, 99, 99, 99)];
        // SAFETY: All ranges are valid and disjoint.
        let status = unsafe {
            aviqtl_timeline_plan_batch_move(
                clips.as_ptr(),
                clips.len(),
                moves.as_ptr(),
                moves.len(),
                [2_i32].as_ptr(),
                1,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert_eq!(status, STATUS_LOCKED_LAYER);
        assert_eq!(output, [clip(99, 99, 99, 99)]);
    }

    #[test]
    fn resize_and_layer_plans_match_edit_ordering() {
        let clips = [clip(1, 0, 0, 10), clip(2, 2, 20, 10), clip(3, 1, 10, 10)];
        let resized = plan_resize(&clips, 1, 4);
        assert_eq!(
            resized.iter().map(|clip| clip.clip_id).collect::<Vec<_>>(),
            [2, 3, 1]
        );
        assert_eq!(resized[0].start_frame, 21);
        assert_eq!(resized[0].duration_frames, 14);

        let inserted = plan_insert_layers(&clips, 1, 2, true).unwrap();
        assert_eq!(
            inserted.iter().map(|clip| clip.clip_id).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(inserted[0].layer, 4);
        assert_eq!(inserted[1].layer, 3);

        let shifted = plan_shift_layers(&clips, 1, 2, -1).unwrap();
        assert_eq!(
            shifted.iter().map(|clip| clip.clip_id).collect::<Vec<_>>(),
            [3, 2]
        );
        assert_eq!(shifted[0].layer, 0);
        assert_eq!(shifted[1].layer, 1);
    }

    #[test]
    fn clipboard_span_and_vacancy_preserve_relative_layout() {
        let clipboard = [clip(1, 2, 20, 10), clip(2, 4, 40, 5)];
        assert_eq!(clipboard_duration(&clipboard), 25);
        let existing = [clip(3, 0, 15, 10)];
        assert_eq!(
            find_vacant_clipboard_frame(&existing, &clipboard, 10, 0),
            Ok(25)
        );

        let (safe_frame, planned) = plan_clipboard_placement(&existing, &clipboard, 10, 0).unwrap();
        assert_eq!(safe_frame, 25);
        assert_eq!(planned, [clip(1, 0, 25, 10), clip(2, 2, 45, 5)]);

        let overlapping = [clip(1, 0, 0, 10), clip(2, 0, 5, 10)];
        assert_eq!(
            plan_clipboard_placement(&[], &overlapping, 0, 0),
            Err(STATUS_INVALID_ARGUMENT)
        );
    }

    #[test]
    fn every_plan_rejects_layers_outside_the_domain() {
        let clips = [clip(1, 127, 0, 10)];
        let movement = [AviQtlTimelineMoveInput {
            clip_id: 1,
            old_layer: 127,
            old_start_frame: 0,
            duration_frames: 10,
            target_layer: 128,
            target_start_frame: 0,
        }];
        assert_eq!(
            plan_batch_move(&clips, &movement, &[]),
            Err(STATUS_INVALID_ARGUMENT)
        );
        assert_eq!(
            plan_insert_layers(&clips, 127, 1, true),
            Err(STATUS_INVALID_ARGUMENT)
        );
        assert_eq!(
            plan_shift_layers(&clips, 127, 127, 1),
            Err(STATUS_INVALID_ARGUMENT)
        );
        assert_eq!(
            plan_clipboard_placement(&[], &clips, 0, 128),
            Err(STATUS_INVALID_ARGUMENT)
        );
    }

    #[test]
    fn delta_move_size_query_does_not_partially_write() {
        let clips = [clip(1, 0, 0, 10), clip(2, 1, 20, 10)];
        let ids = [1_i32, 2_i32];
        let mut output = [clip(99, 99, 99, 99)];
        let mut required = 0_usize;
        // SAFETY: Every range is valid and disjoint; the output is intentionally undersized.
        let status = unsafe {
            aviqtl_timeline_plan_delta_move(
                clips.as_ptr(),
                clips.len(),
                ids.as_ptr(),
                ids.len(),
                std::ptr::null(),
                0,
                1,
                5,
                output.as_mut_ptr(),
                output.len(),
                &mut required,
            )
        };
        assert_eq!(status, STATUS_BUFFER_TOO_SMALL);
        assert_eq!(required, 2);
        assert_eq!(output, [clip(99, 99, 99, 99)]);
    }

    #[test]
    fn split_rejects_edges_without_partial_writes() {
        let source = clip(1, 2, 10, 20);
        let mut first = clip(98, 98, 98, 98);
        let mut second = clip(99, 99, 99, 99);
        // SAFETY: All ranges are valid and disjoint.
        let status = unsafe { aviqtl_timeline_split_clip(&source, 10, &mut first, &mut second) };
        assert_eq!(status, STATUS_INVALID_ARGUMENT);
        assert_eq!(first, clip(98, 98, 98, 98));
        assert_eq!(second, clip(99, 99, 99, 99));

        // SAFETY: All ranges are valid and disjoint.
        let status = unsafe { aviqtl_timeline_split_clip(&source, 18, &mut first, &mut second) };
        assert_eq!(status, STATUS_OK);
        assert_eq!(first.duration_frames, 8);
        assert_eq!(second.start_frame, 18);
        assert_eq!(second.duration_frames, 12);
    }
}
