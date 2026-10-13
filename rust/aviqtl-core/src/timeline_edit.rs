use crate::abi::{
    AviQtlTimelineClipGeometry, AviQtlTimelineMoveInput, AviQtlTimelinePosition,
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_LOCKED_LAYER, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use crate::project::MAX_TIMELINE_LAYER;

const MIN_LAYER: i32 = 0;
const MAX_LAYER: i32 = MAX_TIMELINE_LAYER;

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

pub(crate) fn find_vacant_frame(
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

pub(crate) fn plan_delta_move(
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

pub(crate) fn plan_resize(
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

pub(crate) fn plan_insert_layers(
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

pub(crate) fn plan_shift_layers(
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

pub(crate) fn clipboard_duration(clips: &[AviQtlTimelineClipGeometry]) -> i32 {
    let Some(minimum_start) = clips.iter().map(|clip| i64::from(clip.start_frame)).min() else {
        return 0;
    };
    let maximum_end = clips.iter().map(clip_end).max().unwrap_or(minimum_start);
    clamp_i64(maximum_end.saturating_sub(minimum_start).max(0))
}

/// Which edge of a clip a trim tool acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrimEdge {
    Start,
    End,
}

/// Finds the clip that abuts `edge` of `clip` on the same layer, which is the neighbour a roll
/// moves along with. Only a clip whose end equals the anchor's start, or whose start equals the
/// anchor's end, counts as adjacent.
fn find_adjacent(
    clips: &[AviQtlTimelineClipGeometry],
    clip: &AviQtlTimelineClipGeometry,
    edge: TrimEdge,
) -> Option<AviQtlTimelineClipGeometry> {
    let anchor_start = i64::from(clip.start_frame);
    let anchor_end = clip_end(clip);
    clips.iter().copied().find(|candidate| {
        candidate.clip_id != clip.clip_id
            && candidate.layer == clip.layer
            && match edge {
                TrimEdge::Start => clip_end(candidate) == anchor_start,
                TrimEdge::End => i64::from(candidate.start_frame) == anchor_end,
            }
    })
}

/// Plans a **ripple** trim: the dragged edge moves, and everything strictly after the clip on
/// the same layer shifts by the amount the clip's length changed, so no gap opens.
///
/// Only clips whose geometry actually changes are returned, so a caller applying the plan as a
/// diff never rewrites untouched clips. The trim keeps the anchor's positive-duration floor, and
/// a tail shift that would collide is rejected rather than silently overwriting a clip.
pub(crate) fn plan_ripple_trim(
    clips: &[AviQtlTimelineClipGeometry],
    clip_id: i32,
    edge: TrimEdge,
    delta_frames: i32,
    minimum_duration_frames: i32,
) -> Result<Vec<AviQtlTimelineClipGeometry>, u32> {
    let Some(anchor) = clips.iter().copied().find(|clip| clip.clip_id == clip_id) else {
        return Err(STATUS_INVALID_ARGUMENT);
    };
    if delta_frames == 0 {
        return Ok(Vec::new());
    }
    let minimum = minimum_duration_frames.max(1);
    let anchor_start = i64::from(anchor.start_frame);
    let anchor_end = clip_end(&anchor);

    // The dragged edge moves by the requested delta, bounded so the clip keeps at least the
    // minimum duration. For the start edge the clip's end stays put, so only its start moves.
    let (trimmed_start, trimmed_duration) = match edge {
        TrimEdge::Start => {
            let latest = anchor_end - i64::from(minimum);
            let delta = i64::from(delta_frames)
                .min(latest - anchor_start)
                .max(-anchor_start);
            (
                clamp_i64(anchor_start + delta),
                (anchor_end - anchor_start - delta) as i32,
            )
        }
        TrimEdge::End => {
            let earliest = anchor_start + i64::from(minimum);
            let delta = i64::from(delta_frames).max(earliest - anchor_end);
            (
                anchor.start_frame,
                (anchor_end + delta - anchor_start) as i32,
            )
        }
    };
    if trimmed_duration < minimum {
        return Err(STATUS_INVALID_ARGUMENT);
    }

    // The tail follows the anchor's trimmed end, not its length change: a start trim keeps the
    // end in place, so the tail stays put and no gap or overlap opens, while an end trim moves
    // the end and the tail closes the resulting gap. The collision boundary starts at that same
    // trimmed end, so a shifted clip cannot run into the anchor or its predecessor.
    let trimmed_end = i64::from(trimmed_start) + i64::from(trimmed_duration);
    let change = trimmed_end - anchor_end;
    let mut planned = vec![AviQtlTimelineClipGeometry {
        clip_id: anchor.clip_id,
        layer: anchor.layer,
        start_frame: trimmed_start,
        duration_frames: trimmed_duration,
    }];
    if change == 0 {
        return Ok(planned);
    }

    // Everything strictly after the clip on the layer moves by the same delta. The tail is
    // validated in order so a shift cannot run a clip into its predecessor.
    let mut tail: Vec<_> = clips
        .iter()
        .copied()
        .filter(|clip| {
            clip.layer == anchor.layer
                && i64::from(clip.start_frame) >= anchor_end
                && clip.clip_id != anchor.clip_id
        })
        .collect();
    tail.sort_by_key(|clip| clip.start_frame);
    let mut boundary = trimmed_end;
    for clip in tail {
        let start = i64::from(clip.start_frame) + change;
        if start < boundary {
            return Err(STATUS_INVALID_ARGUMENT);
        }
        planned.push(AviQtlTimelineClipGeometry {
            clip_id: clip.clip_id,
            layer: clip.layer,
            start_frame: clamp_i64(start),
            duration_frames: clip.duration_frames,
        });
        boundary = start + i64::from(clip.duration_frames.max(0));
    }
    Ok(planned)
}

/// Plans a **roll**: the shared cut between two adjacent clips moves. Both clips keep their
/// outer edges, so the cut shifts by `delta_frames` and each side changes duration by the same
/// amount in opposite directions.
pub(crate) fn plan_roll(
    clips: &[AviQtlTimelineClipGeometry],
    clip_id: i32,
    edge: TrimEdge,
    delta_frames: i32,
    minimum_duration_frames: i32,
) -> Result<Vec<AviQtlTimelineClipGeometry>, u32> {
    if delta_frames == 0 {
        return Ok(Vec::new());
    }
    let Some(anchor) = clips.iter().copied().find(|clip| clip.clip_id == clip_id) else {
        return Err(STATUS_INVALID_ARGUMENT);
    };
    let Some(neighbour) = find_adjacent(clips, &anchor, edge) else {
        return Err(STATUS_INVALID_ARGUMENT);
    };
    let minimum = minimum_duration_frames.max(1);
    let delta = i64::from(delta_frames);

    // Moving the cut later grows the earlier clip and shrinks the later one, and vice versa.
    let new_cut = match edge {
        TrimEdge::Start => clip_end(&neighbour) + delta,
        TrimEdge::End => clip_end(&anchor) + delta,
    };
    let (earlier, later) = if edge == TrimEdge::Start {
        (neighbour, anchor)
    } else {
        (anchor, neighbour)
    };
    let new_earlier_duration = new_cut - i64::from(earlier.start_frame);
    let new_later_duration = clip_end(&later) - new_cut;
    if new_earlier_duration < i64::from(minimum) || new_later_duration < i64::from(minimum) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    if new_cut < 0 {
        return Err(STATUS_INVALID_ARGUMENT);
    }

    Ok(vec![
        AviQtlTimelineClipGeometry {
            clip_id: earlier.clip_id,
            layer: earlier.layer,
            start_frame: earlier.start_frame,
            duration_frames: clamp_i64(new_earlier_duration),
        },
        AviQtlTimelineClipGeometry {
            clip_id: later.clip_id,
            layer: later.layer,
            start_frame: clamp_i64(new_cut),
            duration_frames: clamp_i64(new_later_duration),
        },
    ])
}

/// Plans a **slide**: the clip keeps its duration and its source range, but moves in time. The
/// clip before it grows to fill the vacated gap and the clip after it shrinks, so neither gap
/// changes.
pub(crate) fn plan_slide(
    clips: &[AviQtlTimelineClipGeometry],
    clip_id: i32,
    delta_frames: i32,
) -> Result<Vec<AviQtlTimelineClipGeometry>, u32> {
    if delta_frames == 0 {
        return Ok(Vec::new());
    }
    let Some(anchor) = clips.iter().copied().find(|clip| clip.clip_id == clip_id) else {
        return Err(STATUS_INVALID_ARGUMENT);
    };
    let Some(previous) = find_adjacent(clips, &anchor, TrimEdge::Start) else {
        return Err(STATUS_INVALID_ARGUMENT);
    };
    let Some(next) = find_adjacent(clips, &anchor, TrimEdge::End) else {
        return Err(STATUS_INVALID_ARGUMENT);
    };
    let delta = i64::from(delta_frames);
    // The slide is bounded by the row of neighbours on both sides: the previous clip and the next
    // clip must each keep at least one frame, because a zero-length clip cannot be placed on the
    // timeline at all. So the anchor's new start stays after the previous clip's start and its new
    // end stays at least one frame before the next clip's end.
    let earliest_start = i64::from(previous.start_frame) + 1;
    let latest_end = clip_end(&next) - 1;
    let latest_start = latest_end - i64::from(anchor.duration_frames);
    // A row with no room at all leaves the anchor pinned between two one-frame neighbours. There
    // is nothing to move, so the planner reports no change rather than an error, and no undo entry
    // is created for a gesture that does nothing.
    if latest_start < earliest_start
        || (i64::from(anchor.start_frame) + delta)
            .clamp(earliest_start, latest_start.max(earliest_start))
            == i64::from(anchor.start_frame)
    {
        return Ok(Vec::new());
    }
    let start = (i64::from(anchor.start_frame) + delta).clamp(earliest_start, latest_start);
    if start < earliest_start {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    let end = start + i64::from(anchor.duration_frames);
    Ok(vec![
        AviQtlTimelineClipGeometry {
            clip_id: previous.clip_id,
            layer: previous.layer,
            start_frame: previous.start_frame,
            duration_frames: clamp_i64(start - i64::from(previous.start_frame)),
        },
        AviQtlTimelineClipGeometry {
            clip_id: anchor.clip_id,
            layer: anchor.layer,
            start_frame: clamp_i64(start),
            duration_frames: anchor.duration_frames,
        },
        AviQtlTimelineClipGeometry {
            clip_id: next.clip_id,
            layer: next.layer,
            start_frame: clamp_i64(end),
            duration_frames: clamp_i64(clip_end(&next) - end),
        },
    ])
}

/// Plans a **slip**: the clip keeps its timeline span and duration, only its source in-point
/// moves.
///
/// The new in-point is derived from the caller's current in-point and the requested delta and is
/// then bounded to `0 <= in <= source_duration - clip_duration`, so the selected range always
/// stays inside the source. The caller must supply the source length; without one there is no
/// safe bound, so the planner reports no change rather than guessing a range.
pub(crate) fn plan_slip(
    current_source_start_frames: i32,
    delta_frames: i32,
    clip_duration_frames: i32,
    source_duration_frames: Option<i32>,
) -> Option<i32> {
    if delta_frames == 0 {
        return None;
    }
    let source_duration = source_duration_frames.filter(|frames| *frames > 0)?;
    let clip_frames = clip_duration_frames.clamp(1, i32::MAX);
    let latest_start = source_duration.saturating_sub(clip_frames);
    let new_start = i64::from(current_source_start_frames) + i64::from(delta_frames);
    // A delta that would run off either end of the source is clamped to the nearest legal
    // in-point, which is the standard slip behaviour.
    let clamped = new_start.clamp(0, i64::from(latest_start.max(0)));
    if clamped == i64::from(current_source_start_frames) {
        return None;
    }
    Some(clamp_i64(clamped))
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

pub(crate) fn plan_clipboard_placement(
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
        let clips = [clip(1, MAX_LAYER, 0, 10)];
        let movement = [AviQtlTimelineMoveInput {
            clip_id: 1,
            old_layer: MAX_LAYER,
            old_start_frame: 0,
            duration_frames: 10,
            target_layer: MAX_LAYER + 1,
            target_start_frame: 0,
        }];
        assert_eq!(
            plan_batch_move(&clips, &movement, &[]),
            Err(STATUS_INVALID_ARGUMENT)
        );
        assert_eq!(
            plan_insert_layers(&clips, MAX_LAYER, 1, true),
            Err(STATUS_INVALID_ARGUMENT)
        );
        assert_eq!(
            plan_shift_layers(&clips, MAX_LAYER, MAX_LAYER, 1),
            Err(STATUS_INVALID_ARGUMENT)
        );
        assert_eq!(
            plan_clipboard_placement(&[], &clips, 0, MAX_LAYER + 1),
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

    fn planned(clips: &[AviQtlTimelineClipGeometry]) -> Vec<(i32, i32, i32)> {
        clips
            .iter()
            .map(|clip| (clip.clip_id, clip.start_frame, clip.duration_frames))
            .collect()
    }

    #[test]
    fn ripple_trim_closes_the_gap_behind_the_anchor() {
        // Two clips precede the anchor on the layer; the second follows its original end.
        let clips = [clip(1, 0, 0, 10), clip(3, 0, 10, 20), clip(4, 0, 30, 10)];

        // Shortening the anchor by 5 shifts the tail earlier, so no gap opens. Clip 1 is not
        // touched, so the planner reports only the clips it changed.
        assert_eq!(
            planned(&plan_ripple_trim(&clips, 3, TrimEdge::End, -5, 1).expect("ripple plans")),
            [(3, 10, 15), (4, 25, 10)]
        );

        // Lengthening the anchor by 5 shifts the tail later.
        assert_eq!(
            planned(&plan_ripple_trim(&clips, 3, TrimEdge::End, 5, 1).expect("ripple plans")),
            [(3, 10, 25), (4, 35, 10)]
        );
    }

    #[test]
    fn ripple_trim_from_the_start_edge_keeps_the_tail_in_place() {
        let clips = [clip(1, 0, 0, 10), clip(3, 0, 10, 20), clip(4, 0, 30, 10)];
        // Trimming the start by 4 moves the anchor's start to 14 while its end stays at 30, so the
        // tail, which already abutted that end, does not move: only the anchor changes.
        assert_eq!(
            planned(&plan_ripple_trim(&clips, 3, TrimEdge::Start, 4, 1).expect("ripple plans")),
            [(3, 14, 16)]
        );

        // The start edge is bounded by the anchor's own end, so a trim that would empty the clip
        // is clamped to the minimum duration; the end stays put and the tail does not move.
        assert_eq!(
            planned(&plan_ripple_trim(&clips, 3, TrimEdge::Start, 19, 1).expect("clamped")),
            [(3, 29, 1)]
        );
        assert!(plan_ripple_trim(&clips, 3, TrimEdge::Start, -40, 1).is_ok());

        // The lower bound is frame zero, so trimming past the row's first frame is clamped and
        // the tail does not move at all.
        assert_eq!(
            planned(&plan_ripple_trim(&clips, 1, TrimEdge::Start, -5, 1).expect("clamped")),
            [(1, 0, 10)]
        );
    }

    #[test]
    fn ripple_trim_rejects_unknown_clips_and_no_ops() {
        let clips = [clip(1, 0, 0, 10), clip(3, 0, 10, 20)];
        assert!(plan_ripple_trim(&clips, 99, TrimEdge::End, 5, 1).is_err());
        assert!(
            plan_ripple_trim(&clips, 3, TrimEdge::End, 0, 1)
                .unwrap()
                .is_empty()
        );

        // Growing the anchor pushes the tail into its predecessor's end, so the plan is
        // rejected rather than silently overlapping.
        let colliding = [clip(1, 0, 0, 10), clip(2, 0, 10, 20), clip(3, 0, 29, 10)];
        assert!(plan_ripple_trim(&colliding, 1, TrimEdge::End, 5, 1).is_err());
        // The same row without the overlap is accepted.
        let roomy = [clip(1, 0, 0, 10), clip(2, 0, 10, 20), clip(3, 0, 40, 10)];
        assert!(plan_ripple_trim(&roomy, 1, TrimEdge::End, 5, 1).is_ok());
    }

    #[test]
    fn roll_moves_the_shared_cut_and_keeps_outer_edges() {
        // Clip 3 sits between two neighbours on the layer.
        let clips = [clip(2, 0, 0, 10), clip(3, 0, 10, 20), clip(4, 0, 30, 10)];

        // Rolling the start later moves the cut right: the previous clip grows, the anchor shrinks.
        assert_eq!(
            planned(&plan_roll(&clips, 3, TrimEdge::Start, 4, 1).expect("roll plans")),
            [(2, 0, 14), (3, 14, 16)]
        );

        // Rolling the end later moves the right cut right: the anchor grows, the next shrinks.
        assert_eq!(
            planned(&plan_roll(&clips, 3, TrimEdge::End, 4, 1).expect("roll plans")),
            [(3, 10, 24), (4, 34, 6)]
        );
    }

    #[test]
    fn roll_rejects_a_cut_without_a_neighbour_and_below_minimum_duration() {
        let no_left = [clip(2, 0, 10, 20), clip(3, 0, 30, 10)];
        assert!(plan_roll(&no_left, 2, TrimEdge::Start, 4, 1).is_err());

        // Neither neighbour may be squeezed below the minimum duration.
        let clips = [clip(1, 0, 0, 3), clip(2, 0, 3, 20), clip(3, 0, 23, 10)];
        assert!(plan_roll(&clips, 2, TrimEdge::Start, -3, 5).is_err());
        assert!(plan_roll(&clips, 2, TrimEdge::End, 60, 1).is_err());
        assert!(plan_roll(&clips, 99, TrimEdge::End, 4, 1).is_err());
        assert!(
            plan_roll(&clips, 2, TrimEdge::End, 0, 1)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn slide_keeps_duration_and_absorbs_the_move_in_both_neighbours() {
        // Clip 2 has both neighbours adjacent on the layer.
        let clips = [clip(1, 0, 0, 10), clip(2, 0, 10, 20), clip(3, 0, 30, 10)];

        // Sliding 4 frames later grows the previous clip and shrinks the next one; the total
        // span from 0 to 40 is unchanged.
        assert_eq!(
            planned(&plan_slide(&clips, 2, 4).expect("slide plans")),
            [(1, 0, 14), (2, 14, 20), (3, 34, 6)]
        );

        // Sliding earlier is the mirror image.
        assert_eq!(
            planned(&plan_slide(&clips, 2, -4).expect("slide plans")),
            [(1, 0, 6), (2, 6, 20), (3, 26, 14)]
        );
    }

    #[test]
    fn slide_rejects_a_move_without_both_neighbours_or_out_of_bounds() {
        // Missing the left neighbour.
        let missing_left = [clip(2, 0, 10, 20), clip(3, 0, 30, 10)];
        assert!(plan_slide(&missing_left, 2, 4).is_err());

        // Missing the right neighbour.
        let missing_right = [clip(1, 0, 0, 10), clip(2, 0, 10, 20)];
        assert!(plan_slide(&missing_right, 2, 4).is_err());

        // Neither end may leave the row of neighbours: a slide that would push the clip before
        // the previous clip's start, or past the next clip's end, is clamped so that both
        // neighbours keep at least one frame.
        let clips = [clip(1, 0, 0, 10), clip(2, 0, 10, 20), clip(3, 0, 30, 10)];
        assert_eq!(
            planned(&plan_slide(&clips, 2, -6).expect("clamped to the previous clip")),
            [(1, 0, 4), (2, 4, 20), (3, 24, 16)]
        );
        assert_eq!(
            planned(&plan_slide(&clips, 2, 6).expect("clamped to the next clip")),
            [(1, 0, 16), (2, 16, 20), (3, 36, 4)]
        );
        assert_eq!(
            planned(&plan_slide(&clips, 2, -99).expect("clamped to the previous clip")),
            [(1, 0, 1), (2, 1, 20), (3, 21, 19)]
        );
        assert_eq!(
            planned(&plan_slide(&clips, 2, 99).expect("clamped to the next clip")),
            [(1, 0, 19), (2, 19, 20), (3, 39, 1)]
        );
        assert!(plan_slide(&clips, 99, 4).is_err(), "unknown clip");
        assert!(plan_slide(&clips, 2, 0).unwrap().is_empty(), "no-op");

        // A row where the anchor is exactly as long as the space between its neighbours still has
        // room, because only one frame has to survive on each side.
        let tight = [clip(1, 0, 0, 10), clip(2, 0, 10, 10), clip(3, 0, 20, 10)];
        assert_eq!(
            planned(&plan_slide(&tight, 2, 1).expect("one frame of room on each side")),
            [(1, 0, 11), (2, 11, 10), (3, 21, 9)]
        );

        // A row with no room at all pins the anchor between two one-frame neighbours. The
        // previous clip starts one frame before the anchor and the next clip ends one frame after
        // it, so there is nothing to move and the planner reports no change rather than an error,
        // which also keeps a wedged row from pushing an undo entry.
        let wedged = [clip(1, 0, 9, 1), clip(2, 0, 10, 20), clip(3, 0, 30, 1)];
        assert!(
            plan_slide(&wedged, 2, 1)
                .expect("a wedged row is not an error")
                .is_empty()
        );
        assert!(
            plan_slide(&wedged, 2, -1)
                .expect("a wedged row is not an error")
                .is_empty()
        );
        assert!(
            plan_slide(&tight, 2, 0).unwrap().is_empty(),
            "no-op is still a no-op"
        );
    }

    #[test]
    fn slip_is_a_no_op_without_a_known_source_duration() {
        // Without a source length there is no safe bound, so the tool reports no change.
        assert_eq!(plan_slip(0, 5, 20, None), None);
        assert_eq!(plan_slip(0, 5, 20, Some(0)), None);
        assert_eq!(plan_slip(0, 0, 20, Some(100)), None);
    }

    #[test]
    fn slip_bounds_the_new_in_point_inside_the_source() {
        // A 20-frame clip on a 40-frame source may start between 0 and 20.
        assert_eq!(plan_slip(0, 10, 20, Some(40)), Some(10));
        // Running past the end of the source clamps to the last legal in-point.
        assert_eq!(plan_slip(0, 21, 20, Some(40)), Some(20));
        // Running before the start clamps to zero.
        assert_eq!(plan_slip(5, -10, 20, Some(40)), Some(0));
        // Already at the bound, the move changes nothing.
        assert_eq!(plan_slip(0, -1, 20, Some(40)), None);
        assert_eq!(plan_slip(20, 1, 20, Some(40)), None);
    }
}
