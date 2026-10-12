//! Object-level midpoints, the AviUtl ExEdit `P` boundary.
//!
//! A midpoint is a clip-internal time boundary. Unlike a [`KeyframeTrack`](crate::api::KeyframePoint),
//! which belongs to one effect parameter, a midpoint belongs to the object: it divides the clip
//! into independently parameterised halves. ExEdit implements it by adding a keyframe at that
//! frame to every animatable parameter, so the whole object changes parameterisation at once.
//!
//! This module owns the clip-level list only. It deliberately does not create or shear
//! parameter tracks; see `docs/batch4-design.md` section 1 for why that half of the feature is
//! a separate step that needs the desktop suite.

use serde_json::Value;
use std::collections::BTreeMap;

/// Persisted key on `ClipDocument::extra`. Absent means the clip has never had a midpoint.
pub const MIDPOINTS_KEY: &str = "midpoints";

/// The flattened extras map a project document carries alongside its known fields.
pub(crate) type Extras = BTreeMap<String, Value>;

/// A validated, strictly increasing list of clip-relative midpoint frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Midpoints {
    frames: Vec<i32>,
}

impl Midpoints {
    /// Builds a list from already-classified frames. Callers pass frames this module produced;
    /// [`sanitize`](Self::sanitize) is what enforces the open interval, so the ordering
    /// invariant is re-established before the value is persisted.
    pub(crate) fn from_frames(mut frames: Vec<i32>) -> Self {
        frames.sort_unstable();
        frames.dedup();
        Self { frames }
    }

    /// Returns the frames in ascending order.
    pub(crate) fn frames(&self) -> &[i32] {
        &self.frames
    }

    /// Returns the midpoint interval containing `frame`, as a half-open `(lower, upper)` pair
    /// where both bounds are fixed frames of the clip. A frame that *is* a midpoint opens its own
    /// segment, so it is its own lower bound.
    pub(crate) fn interval(&self, frame: i32) -> (i32, i32) {
        let lower = self
            .frames
            .partition_point(|midpoint| *midpoint <= frame)
            .checked_sub(1)
            .map(|index| self.frames[index])
            .unwrap_or(0);
        let upper = self
            .frames
            .iter()
            .copied()
            .find(|midpoint| *midpoint > frame)
            .unwrap_or(i32::MAX);
        (lower, upper)
    }

    /// Returns the number of parameterisation segments a clip with these midpoints has. One
    /// midpoint divides a clip into two segments.
    pub(crate) fn segment_count(&self, duration: i32) -> usize {
        self.frames
            .iter()
            .filter(|frame| **frame > 0 && **frame < duration)
            .count()
            + 1
    }
}

/// Reads the midpoint list from a clip's flattened extras, dropping any frame that is not a
/// valid midpoint. Unparseable content yields no midpoints rather than an error, so a corrupt
/// or hand-edited project still loads.
pub(crate) fn read_midpoints(extra: &Extras) -> Midpoints {
    let Some(frames) = extra.get(MIDPOINTS_KEY).and_then(Value::as_array) else {
        return Midpoints::default();
    };
    let mut parsed: Vec<i32> = frames.iter().filter_map(frame_as_i32).collect();
    parsed.sort_unstable();
    parsed.dedup();
    Midpoints { frames: parsed }
}

fn frame_as_i32(value: &Value) -> Option<i32> {
    if let Some(frame) = value.as_i64().and_then(|frame| i32::try_from(frame).ok()) {
        return Some(frame);
    }
    if let Some(frame) = value.as_f64()
        && frame.is_finite()
        && frame >= f64::from(i32::MIN)
        && frame <= f64::from(i32::MAX)
    {
        return Some(frame as i32);
    }
    value.as_str()?.parse::<i32>().ok()
}

/// Writes the midpoint list into a clip's flattened extras, or removes the key when the clip
/// has none. Keeping the key absent is what makes a project that never used midpoints stay
/// comparable with its Qt form.
pub(crate) fn write_midpoints(extra: &mut Extras, midpoints: &Midpoints) {
    if midpoints.frames.is_empty() {
        extra.remove(MIDPOINTS_KEY);
        return;
    }
    extra.insert(
        MIDPOINTS_KEY.to_owned(),
        Value::Array(
            midpoints
                .frames
                .iter()
                .map(|frame| Value::from(*frame))
                .collect(),
        ),
    );
}

/// Rejects any frame that is not strictly inside the clip, then sorts and deduplicates.
pub(crate) fn sanitize(midpoints: Midpoints, duration: i32) -> Midpoints {
    let mut frames: Vec<i32> = midpoints
        .frames
        .into_iter()
        .filter(|frame| *frame > 0 && *frame < duration.max(0))
        .collect();
    frames.sort_unstable();
    frames.dedup();
    Midpoints { frames }
}

/// Adds one midpoint. Returns `None` when the frame is outside the open interval or already
/// present, so a rejected request never produces an undo entry.
pub(crate) fn add(midpoints: &Midpoints, frame: i32, duration: i32) -> Option<Midpoints> {
    if frame <= 0 || frame >= duration.max(0) || midpoints.frames.contains(&frame) {
        return None;
    }
    let mut frames = midpoints.frames.clone();
    frames.push(frame);
    frames.sort_unstable();
    Some(Midpoints { frames })
}

/// Removes one midpoint. Returns `None` when it is not present.
pub(crate) fn remove(midpoints: &Midpoints, frame: i32) -> Option<Midpoints> {
    if !midpoints.frames.contains(&frame) {
        return None;
    }
    Some(Midpoints {
        frames: midpoints
            .frames
            .iter()
            .copied()
            .filter(|existing| *existing != frame)
            .collect(),
    })
}

/// Moves one midpoint. Returns `None` when the old frame is absent, the new frame is outside
/// the open interval, or the move would land on or cross a neighbour. Frames stay strictly
/// increasing; a move is never silently reordered.
pub(crate) fn move_midpoint(
    midpoints: &Midpoints,
    old_frame: i32,
    new_frame: i32,
    duration: i32,
) -> Option<Midpoints> {
    if old_frame == new_frame {
        return None;
    }
    if new_frame <= 0 || new_frame >= duration.max(0) {
        return None;
    }
    let index = midpoints
        .frames
        .iter()
        .position(|frame| *frame == old_frame)?;
    if midpoints
        .frames
        .iter()
        .any(|frame| *frame == new_frame && *frame != old_frame)
    {
        return None;
    }
    if let Some(previous) = index.checked_sub(1).map(|index| midpoints.frames[index])
        && new_frame <= previous
    {
        return None;
    }
    if let Some(next) = midpoints.frames.get(index + 1).copied()
        && new_frame >= next
    {
        return None;
    }
    let mut frames = midpoints.frames.clone();
    frames[index] = new_frame;
    frames.sort_unstable();
    Some(Midpoints { frames })
}

/// Shifts every midpoint by the caller-provided origin delta.
///
/// The caller computes `delta_start` from the origin move it is compensating for: a start trim
/// moves the clip origin by `clip.start - before.start`, so the midpoint shifts by the opposite
/// amount (`before.start - clip.start`) to stay on the same timeline moment. The caller clamps
/// the result against the clip's new span; a midpoint pushed onto a fixed frame is dropped by
/// [`sanitize`].
pub(crate) fn shift_by_start_delta(midpoints: &Midpoints, delta_start: i32) -> Midpoints {
    Midpoints {
        frames: midpoints
            .frames
            .iter()
            .map(|frame| frame.saturating_add(delta_start))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn extra(frames: Value) -> Extras {
        BTreeMap::from_iter([(MIDPOINTS_KEY.to_owned(), frames)])
    }

    #[test]
    fn absent_key_reads_as_no_midpoints() {
        assert!(read_midpoints(&Extras::new()).frames().is_empty());
    }

    #[test]
    fn corrupt_content_reads_as_no_midpoints_rather_than_failing() {
        assert!(
            read_midpoints(&extra(json!("nonsense")))
                .frames()
                .is_empty()
        );
        assert!(
            read_midpoints(&BTreeMap::from_iter([(
                MIDPOINTS_KEY.to_owned(),
                json!({"not": "an array"})
            )]))
            .frames()
            .is_empty()
        );
    }

    #[test]
    fn read_sorts_deduplicates_and_accepts_string_frames() {
        let midpoints = read_midpoints(&extra(json!([30, 10, 30, "20"])));
        assert_eq!(midpoints.frames(), [10, 20, 30]);
    }

    #[test]
    fn write_removes_the_key_when_the_list_is_empty() {
        let mut storage = extra(json!([10]));
        write_midpoints(&mut storage, &Midpoints::default());
        assert!(!storage.contains_key(MIDPOINTS_KEY));

        write_midpoints(&mut storage, &read_midpoints(&extra(json!([5, 15]))));
        assert_eq!(storage[MIDPOINTS_KEY], json!([5, 15]));
    }

    #[test]
    fn add_rejects_fixed_frames_duplicates_and_out_of_range() {
        let empty = Midpoints::default();
        assert_eq!(add(&empty, 10, 100).expect("valid").frames(), [10]);
        assert!(add(&empty, 0, 100).is_none());
        assert!(add(&empty, 100, 100).is_none());
        assert!(add(&empty, -1, 100).is_none());
        assert!(add(&empty, 10, 10).is_none());

        let existing = read_midpoints(&extra(json!([10])));
        assert!(add(&existing, 10, 100).is_none());
        assert_eq!(add(&existing, 50, 100).expect("valid").frames(), [10, 50]);
    }

    #[test]
    fn remove_reports_absence_and_removes_the_last_key() {
        let midpoints = read_midpoints(&extra(json!([10, 50])));
        assert!(remove(&midpoints, 30).is_none());
        assert_eq!(remove(&midpoints, 10).expect("present").frames(), [50]);
        assert!(
            remove(&read_midpoints(&extra(json!([10]))), 10)
                .expect("present")
                .frames()
                .is_empty()
        );
    }

    #[test]
    fn move_rejects_crossing_a_neighbour_or_landing_on_one() {
        let midpoints = read_midpoints(&extra(json!([20, 40])));
        assert_eq!(
            move_midpoint(&midpoints, 20, 30, 100)
                .expect("inside")
                .frames(),
            [30, 40]
        );
        assert!(
            move_midpoint(&midpoints, 20, 40, 100).is_none(),
            "onto next"
        );
        assert!(
            move_midpoint(&midpoints, 40, 20, 100).is_none(),
            "onto previous"
        );
        assert!(move_midpoint(&midpoints, 20, 20, 100).is_none(), "no-op");
        assert!(
            move_midpoint(&midpoints, 20, 0, 100).is_none(),
            "frame zero"
        );
        assert!(
            move_midpoint(&midpoints, 20, 100, 100).is_none(),
            "endpoint"
        );
        assert!(
            move_midpoint(&midpoints, 99, 10, 100).is_none(),
            "absent old"
        );
        // The first midpoint's only lower neighbour is the fixed frame 0, so moving it to 10 is
        // inside its neighbour-free space rather than a crossing.
        assert_eq!(
            move_midpoint(&midpoints, 20, 10, 100)
                .expect("inside")
                .frames(),
            [10, 40]
        );
        // A move may not leave the slot bounded by its immediate neighbours, so the middle
        // midpoint of a three-element list can only move within (20, 60).
        let three = read_midpoints(&extra(json!([20, 40, 60])));
        assert_eq!(
            move_midpoint(&three, 40, 45, 100)
                .expect("inside the slot")
                .frames(),
            [20, 45, 60]
        );
        assert_eq!(
            move_midpoint(&three, 40, 30, 100)
                .expect("inside the slot")
                .frames(),
            [20, 30, 60]
        );
        assert!(
            move_midpoint(&three, 40, 60, 100).is_none(),
            "onto the next"
        );
        assert!(
            move_midpoint(&three, 40, 20, 100).is_none(),
            "onto the previous"
        );
    }

    #[test]
    fn interval_reports_the_containing_segment_bounds() {
        let midpoints = read_midpoints(&extra(json!([20, 40])));
        assert_eq!(midpoints.interval(0), (0, 20));
        assert_eq!(midpoints.interval(19), (0, 20));
        assert_eq!(midpoints.interval(20), (20, 40));
        assert_eq!(midpoints.interval(30), (20, 40));
        assert_eq!(midpoints.interval(40), (40, i32::MAX));
        assert_eq!(midpoints.interval(i32::MAX), (40, i32::MAX));
    }

    #[test]
    fn sanitize_drops_frames_outside_the_open_interval() {
        let midpoints = read_midpoints(&extra(json!([-5, 0, 10, 99, 100, 250])));
        assert_eq!(sanitize(midpoints, 100).frames(), [10, 99]);
        assert!(
            sanitize(read_midpoints(&extra(json!([10]))), 10)
                .frames()
                .is_empty()
        );
    }

    #[test]
    fn segment_count_counts_parameterisation_segments() {
        assert_eq!(Midpoints::default().segment_count(100), 1);
        assert_eq!(read_midpoints(&extra(json!([10]))).segment_count(100), 2);
        assert_eq!(
            read_midpoints(&extra(json!([10, 50]))).segment_count(100),
            3
        );
        // A midpoint at or past the endpoint cannot divide anything.
        assert_eq!(read_midpoints(&extra(json!([100]))).segment_count(100), 1);
    }
}
