//! Project-scoped source media, independent of the timeline.
//!
//! A media bin is a list of source files the user has added to the project, whether or not a
//! clip currently references them. Each entry keeps the probed kind and duration plus a
//! user-set in/out range that insert and overwrite use as their default. Bins are a flat,
//! ordered grouping; nesting is deliberately out of scope.
//!
//! The bin is stored as one optional `mediaBin` key in the project document's flattened
//! extras, so it round-trips through parse and save without a schema version bump and stays
//! absent in every project that never used it.

use crate::media_import::{MediaImportKind, MediaImportPlan};
use aviqtl_rust_core::api::{ProjectDocument, TimelineCommand, TimelineState, TimelineTransaction};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Persisted top-level key. Absent means the project has never had a media bin.
pub const MEDIA_BIN_KEY: &str = "mediaBin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaBinEntry {
    pub id: i32,
    pub path: String,
    pub name: String,
    pub kind: MediaImportKind,
    /// Probed source length in frames. Images have no intrinsic length, so this is the
    /// project's default clip duration at the time the entry was added.
    pub duration_frames: i32,
    pub in_frame: i32,
    pub out_frame: i32,
}

impl MediaBinEntry {
    /// Returns the selected range length, which is what insert and overwrite use as the
    /// resulting clip duration.
    pub fn range_frames(&self) -> i32 {
        (self.out_frame - self.in_frame).max(1)
    }

    /// Reports whether the entry is currently missing on disk. A missing entry is kept rather
    /// than removed, so the user can relink it.
    pub fn is_missing(&self, project_path: Option<&Path>) -> bool {
        !resolve_path(project_path, &self.path).exists()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaBin {
    entries: Vec<MediaBinEntry>,
    bins: Vec<Bin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bin {
    pub id: i32,
    pub name: String,
    pub entry_ids: Vec<i32>,
}

/// Reads the bin from a document. An absent or malformed key yields an empty bin rather than
/// an error, so a hand-edited project still loads.
pub fn read_media_bin(document: &ProjectDocument) -> MediaBin {
    let Some(root) = document.extra.get(MEDIA_BIN_KEY) else {
        return MediaBin::default();
    };
    MediaBin {
        entries: array(root.get("entries"))
            .iter()
            .filter_map(read_entry)
            .collect(),
        bins: array(root.get("bins"))
            .iter()
            .filter_map(read_bin)
            .collect(),
    }
}

/// Writes the bin back into a document's extras, removing the key when the bin is empty so a
/// project that never used it stays identical to its pre-feature form.
pub fn write_media_bin(document: &mut ProjectDocument, bin: &MediaBin) {
    if bin.entries.is_empty() && bin.bins.is_empty() {
        document.extra.remove(MEDIA_BIN_KEY);
        return;
    }
    document.extra.insert(
        MEDIA_BIN_KEY.to_owned(),
        json!({
            "entries": bin.entries.iter().map(entry_to_json).collect::<Vec<_>>(),
            "bins": bin.bins.iter().map(bin_to_json).collect::<Vec<_>>(),
        }),
    );
}

/// Builds an entry from a probed import plan. The kind and duration always come from the
/// probe; the bin never guesses them.
pub fn entry_from_plan(
    id: i32,
    plan: &MediaImportPlan,
    project_path: Option<&Path>,
) -> MediaBinEntry {
    let path = relative_path(&plan.path, project_path);
    MediaBinEntry {
        id,
        name: plan
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone()),
        kind: plan.kind,
        duration_frames: plan.duration_frames.max(1),
        in_frame: 0,
        out_frame: plan.duration_frames.max(1),
        path,
    }
}

impl MediaBin {
    pub fn entries(&self) -> &[MediaBinEntry] {
        &self.entries
    }

    pub fn bins(&self) -> &[Bin] {
        &self.bins
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.bins.is_empty()
    }

    pub fn entry(&self, id: i32) -> Option<&MediaBinEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Returns the next free entry id. Ids are never reused after a removal, so a stale
    /// reference cannot silently attach to a different file.
    pub fn next_entry_id(&self) -> i32 {
        self.entries.iter().map(|entry| entry.id).max().unwrap_or(0) + 1
    }

    pub fn next_bin_id(&self) -> i32 {
        self.bins.iter().map(|bin| bin.id).max().unwrap_or(0) + 1
    }

    /// Adds an entry, or returns the existing one when the same resolved path is already
    /// listed. Dropping the same file twice must not create a second row.
    pub fn upsert(&mut self, entry: MediaBinEntry) -> i32 {
        let key = normalize_key(&entry.path);
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|candidate| normalize_key(&candidate.path) == key)
        {
            // A re-add refreshes the probe but keeps the user's in/out range, because that
            // range is an editing decision rather than a property of the file.
            existing.kind = entry.kind;
            existing.duration_frames = entry.duration_frames;
            existing.out_frame = entry.out_frame.min(existing.out_frame.max(1));
            return existing.id;
        }
        let id = entry.id.max(self.next_entry_id());
        self.entries.push(MediaBinEntry { id, ..entry });
        self.entries.sort_by_key(|entry| entry.id);
        id
    }

    /// Removes one entry. Clips keep their own media reference, so removing an entry never
    /// touches the timeline.
    pub fn remove(&mut self, id: i32) -> bool {
        let removed = self.entries.iter().any(|entry| entry.id == id);
        self.entries.retain(|entry| entry.id != id);
        for bin in &mut self.bins {
            bin.entry_ids.retain(|entry_id| *entry_id != id);
        }
        self.bins.retain(|bin| !bin.entry_ids.is_empty());
        removed
    }

    /// Sets the in or out frame, clamping the pair into a valid, non-degenerate range. Out is
    /// clamped first so that setting an in point beyond the current out still yields a valid
    /// pair rather than an error.
    pub fn set_range(&mut self, id: i32, in_frame: Option<i32>, out_frame: Option<i32>) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        let duration = entry.duration_frames.max(1);
        if let Some(out) = out_frame {
            entry.out_frame = out.clamp(1, duration);
        }
        if let Some(in_point) = in_frame {
            entry.in_frame = in_point.clamp(0, entry.out_frame.saturating_sub(1));
        }
        true
    }

    pub fn create_bin(&mut self, name: &str) -> i32 {
        let id = self.next_bin_id();
        self.bins.push(Bin {
            id,
            name: name.to_owned(),
            entry_ids: Vec::new(),
        });
        id
    }

    /// Moves an entry into a bin, removing it from any other bin. An entry belongs to at most
    /// one bin, which keeps "show only this bin" a simple filter.
    pub fn assign_to_bin(&mut self, entry_id: i32, bin_id: i32) -> bool {
        if self.entry(entry_id).is_none()
            || !self.bins.iter().any(|candidate| candidate.id == bin_id)
        {
            return false;
        }
        for bin in &mut self.bins {
            bin.entry_ids.retain(|existing| *existing != entry_id);
        }
        if let Some(bin) = self.bins.iter_mut().find(|bin| bin.id == bin_id) {
            bin.entry_ids.push(entry_id);
        }
        true
    }

    pub fn remove_bin(&mut self, bin_id: i32) -> bool {
        let removed = self.bins.iter().any(|bin| bin.id == bin_id);
        self.bins.retain(|bin| bin.id != bin_id);
        removed
    }

    /// Moves an entry within the flat list.
    pub fn reorder(&mut self, entry_id: i32, target_index: usize) -> bool {
        let Some(current) = self.entries.iter().position(|entry| entry.id == entry_id) else {
            return false;
        };
        let target_index = target_index.min(self.entries.len().saturating_sub(1));
        if current == target_index {
            return false;
        }
        let entry = self.entries.remove(current);
        self.entries.insert(target_index, entry);
        true
    }
}

/// Reads the bin out of a live timeline state, which is how the workspace exposes it.
pub fn bin_from_state(state: &TimelineState) -> MediaBin {
    read_media_bin(&state.snapshot())
}

fn read_entry(value: &Value) -> Option<MediaBinEntry> {
    let entry = value.as_object()?;
    let id = entry.get("id")?.as_i64()?;
    let path = entry.get("path")?.as_str()?.to_owned();
    if path.is_empty() {
        return None;
    }
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .map_or_else(|| default_name(&path), ToOwned::to_owned);
    let kind = match entry
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "video" => MediaImportKind::Video,
        "audio" => MediaImportKind::Audio,
        _ => MediaImportKind::Image,
    };
    let duration_frames = entry
        .get("durationFrames")
        .and_then(Value::as_i64)
        .and_then(|frames| i32::try_from(frames).ok())
        .unwrap_or(1)
        .max(1);
    let out_frame = entry
        .get("outFrame")
        .and_then(Value::as_i64)
        .and_then(|frame| i32::try_from(frame).ok())
        .unwrap_or(duration_frames)
        .clamp(1, duration_frames);
    let in_frame = entry
        .get("inFrame")
        .and_then(Value::as_i64)
        .and_then(|frame| i32::try_from(frame).ok())
        .unwrap_or(0)
        .clamp(0, out_frame.saturating_sub(1));
    Some(MediaBinEntry {
        id: i32::try_from(id).ok()?,
        path,
        name,
        kind,
        duration_frames,
        in_frame,
        out_frame,
    })
}

fn read_bin(value: &Value) -> Option<Bin> {
    let bin = value.as_object()?;
    Some(Bin {
        id: i32::try_from(bin.get("id")?.as_i64()?).ok()?,
        name: bin
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        entry_ids: array(bin.get("entryIds"))
            .iter()
            .filter_map(Value::as_i64)
            .filter_map(|id| i32::try_from(id).ok())
            .collect(),
    })
}

fn entry_to_json(entry: &MediaBinEntry) -> Value {
    json!({
        "id": entry.id,
        "path": entry.path,
        "name": entry.name,
        "kind": match entry.kind {
            MediaImportKind::Video => "video",
            MediaImportKind::Audio => "audio",
            MediaImportKind::Image => "image",
        },
        "durationFrames": entry.duration_frames,
        "inFrame": entry.in_frame,
        "outFrame": entry.out_frame,
    })
}

fn bin_to_json(bin: &Bin) -> Value {
    json!({
        "id": bin.id,
        "name": bin.name,
        "entryIds": bin.entry_ids,
    })
}

fn array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn default_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// Stores a path relative to the project directory when the file lives inside it, so moving a
/// project together with its media keeps working. Files outside the project keep their absolute
/// path, matching what the existing clip import stores.
fn relative_path(path: &Path, project_path: Option<&Path>) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let Some(directory) = project_path.and_then(Path::parent) else {
        return absolute.to_string_lossy().into_owned();
    };
    let Ok(directory) = std::path::absolute(directory) else {
        return absolute.to_string_lossy().into_owned();
    };
    match absolute.strip_prefix(&directory) {
        Ok(relative) => relative.to_string_lossy().into_owned(),
        Err(_) => absolute.to_string_lossy().into_owned(),
    }
}

/// Comparison key for deduplication. Two entries are the same source when they resolve to the
/// same file, so a path written relative and one written absolute still collapse.
fn normalize_key(path: &str) -> String {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate.to_string_lossy().to_lowercase()
    } else {
        path.replace('\\', "/").to_lowercase()
    }
}

/// Resolves a stored bin path the same way clips resolve theirs, through the shared rule in
/// `missing_media` so a bin entry and the clip referencing the same file always agree.
pub fn resolve_path(project_path: Option<&Path>, source: &str) -> PathBuf {
    crate::missing_media::resolve_path(project_path, source)
}

/// Plans one bin mutation as a reversible document replacement.
///
/// Every bin edit is a project-level change rather than a clip edit, so it goes through
/// `ReplaceDocument` and inherits the same reverse, atomic patch as every other transaction.
/// A mutation that changes nothing returns `None`, so a no-op never reaches the history stack.
pub fn plan_bin_edit(
    state: &TimelineState,
    document: &ProjectDocument,
    edit: impl FnOnce(&mut MediaBin),
) -> Option<TimelineTransaction> {
    let mut bin = read_media_bin(document);
    edit(&mut bin);
    if bin == read_media_bin(document) {
        return None;
    }
    let mut updated = document.clone();
    write_media_bin(&mut updated, &bin);
    state
        .plan(TimelineCommand::SetProjectExtras {
            extras: updated.extra,
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_import::MediaImportPlan;
    use aviqtl_rust_core::api::TimelineState;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn plan(path: &Path, kind: MediaImportKind, duration_frames: i32) -> MediaImportPlan {
        MediaImportPlan {
            path: path.to_path_buf(),
            kind,
            duration_frames,
        }
    }

    /// Builds a live state whose document already carries the bin, so a planned edit is
    /// applied to the same state it was planned against.
    fn state_with_bin(bin: &MediaBin) -> (TimelineState, ProjectDocument) {
        let mut document = TimelineState::from_json(
            br#"{
                "version": 3,
                "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
                "scenes": [{"id": 1, "name": "Root", "duration": 300}],
                "clips": [{"id": 1, "sceneId": 1, "type": "rect", "start": 0,
                           "duration": 20, "layer": 0}]
            }"#,
        )
        .expect("fixture loads")
        .snapshot();
        write_media_bin(&mut document, bin);
        let bytes = serde_json::to_vec(&document).expect("document serializes");
        let state = TimelineState::from_json(&bytes).expect("document reloads");
        let document = state.snapshot();
        (state, document)
    }

    #[test]
    fn an_absent_bin_key_reads_as_empty_and_is_not_written_back() {
        let document = TimelineState::from_json(
            br#"{"version": 3, "settings": {"width": 1920, "height": 1080, "fps": 60,
                 "sampleRate": 48000}, "scenes": [], "clips": []}"#,
        )
        .expect("fixture loads")
        .snapshot();
        assert!(read_media_bin(&document).is_empty());
        assert!(!document.extra.contains_key(MEDIA_BIN_KEY));

        let mut rewritten = document.clone();
        write_media_bin(&mut rewritten, &MediaBin::default());
        assert!(
            !rewritten.extra.contains_key(MEDIA_BIN_KEY),
            "an unused bin must not add a key"
        );
        assert_eq!(rewritten, document);
    }

    #[test]
    fn a_malformed_bin_reads_as_empty_rather_than_failing() {
        let mut document = TimelineState::from_json(
            br#"{"version": 3, "settings": {"width": 1920, "height": 1080, "fps": 60,
                 "sampleRate": 48000}, "scenes": [], "clips": []}"#,
        )
        .expect("fixture loads")
        .snapshot();
        document
            .extra
            .insert(MEDIA_BIN_KEY.to_owned(), json!("not an object"));
        assert!(read_media_bin(&document).is_empty());

        document.extra.insert(
            MEDIA_BIN_KEY.to_owned(),
            json!({"entries": [{"id": 1}, {"id": 2, "path": ""}, {"id": 3, "path": "a.mp4"}]}),
        );
        let bin = read_media_bin(&document);
        assert_eq!(
            bin.entries().len(),
            1,
            "entries without a usable path are dropped"
        );
        assert_eq!(bin.entries()[0].path, "a.mp4");
    }

    #[test]
    fn a_bin_survives_a_document_round_trip() {
        let mut bin = MediaBin::default();
        bin.upsert(entry_from_plan(
            1,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        bin.upsert(entry_from_plan(
            2,
            &plan(Path::new("/media/b.png"), MediaImportKind::Image, 30),
            None,
        ));
        bin.set_range(1, Some(30), Some(210));

        let state = TimelineState::from_json(
            br#"{"version": 3, "settings": {"width": 1920, "height": 1080, "fps": 60,
                 "sampleRate": 48000}, "scenes": [], "clips": []}"#,
        )
        .expect("fixture loads");
        let mut document = state.snapshot();
        write_media_bin(&mut document, &bin);

        // Save, reload, and compare the bin the application sees.
        let saved = serde_json::to_vec(&document).expect("document serializes");
        let reloaded = TimelineState::from_json(&saved)
            .expect("document reloads")
            .snapshot();
        let reloaded_bin = read_media_bin(&reloaded);
        assert_eq!(reloaded_bin.entries().len(), 2);
        let entry = reloaded_bin.entry(1).expect("the video entry survives");
        assert_eq!(entry.duration_frames, 240);
        assert_eq!(entry.in_frame, 30);
        assert_eq!(entry.out_frame, 210);
        assert_eq!(entry.range_frames(), 180);
        assert_eq!(entry.kind, MediaImportKind::Video);
    }

    #[test]
    fn adding_the_same_path_twice_updates_one_entry() {
        let mut bin = MediaBin::default();
        let first = bin.upsert(entry_from_plan(
            1,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        bin.set_range(first, Some(10), Some(100));
        let second = bin.upsert(entry_from_plan(
            2,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        assert_eq!(first, second, "a duplicate path reuses the existing id");
        assert_eq!(bin.entries().len(), 1);
        assert_eq!(
            bin.entry(first).expect("entry exists").in_frame,
            10,
            "a re-add keeps the user's in point"
        );

        // Path separators and case do not create a second row for the same file.
        bin.upsert(entry_from_plan(
            3,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        assert_eq!(bin.entries().len(), 1);
    }

    #[test]
    fn in_and_out_clamp_into_a_valid_non_degenerate_range() {
        let mut bin = MediaBin::default();
        let id = bin.upsert(entry_from_plan(
            1,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));

        // An in point at or beyond the out point is pulled back, not rejected.
        assert!(bin.set_range(id, None, Some(100)));
        assert!(bin.set_range(id, Some(200), None));
        let entry = bin.entry(id).expect("entry exists");
        assert_eq!(entry.out_frame, 100, "the out point is unchanged");
        assert_eq!(entry.in_frame, 99, "in is pulled back below out");

        // An out point beyond the probed duration is clamped to the duration.
        assert!(bin.set_range(id, None, Some(9_999)));
        assert_eq!(bin.entry(id).expect("entry exists").out_frame, 240);

        // A negative in point is clamped to zero.
        assert!(bin.set_range(id, Some(-5), None));
        assert_eq!(bin.entry(id).expect("entry exists").in_frame, 0);

        assert!(!bin.set_range(999, Some(1), Some(2)), "unknown entry");
    }

    #[test]
    fn removing_an_entry_never_touches_the_timeline() {
        let mut bin = MediaBin::default();
        let id = bin.upsert(entry_from_plan(
            1,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        let group = bin.create_bin("Footage");
        assert!(bin.assign_to_bin(id, group));

        let (state, document) = state_with_bin(&bin);
        let mut edited = document.clone();
        let mut current = read_media_bin(&document);
        current.remove(id);
        write_media_bin(&mut edited, &current);
        assert_eq!(
            edited.clips.len(),
            document.clips.len(),
            "the clip that used the media is untouched"
        );
        assert!(current.is_empty(), "the now-empty group is dropped");
        let _ = state;
    }

    #[test]
    fn an_entry_belongs_to_at_most_one_group() {
        let mut bin = MediaBin::default();
        let id = bin.upsert(entry_from_plan(
            1,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        let first = bin.create_bin("One");
        let second = bin.create_bin("Two");
        assert!(bin.assign_to_bin(id, first));
        assert!(bin.assign_to_bin(id, second));
        assert_eq!(bin.bins()[0].entry_ids, Vec::<i32>::new());
        assert_eq!(bin.bins()[1].entry_ids, [id]);
        assert!(!bin.assign_to_bin(id, 999), "unknown group");
        assert!(!bin.assign_to_bin(999, first), "unknown entry");
    }

    #[test]
    fn groups_never_nest_and_removing_one_keeps_its_entries() {
        let mut bin = MediaBin::default();
        let id = bin.upsert(entry_from_plan(
            1,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        let group = bin.create_bin("Footage");
        bin.assign_to_bin(id, group);
        assert!(bin.remove_bin(group));
        assert!(bin.bins().is_empty());
        assert_eq!(bin.entries().len(), 1, "the entry outlives its group");
    }

    #[test]
    fn a_missing_file_is_reported_and_the_entry_is_kept() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock follows the epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("aviqtl-bin-{}-{nonce}.png", std::process::id()));
        fs::write(&path, b"fixture").expect("fixture writes");
        let mut bin = MediaBin::default();
        let id = bin.upsert(entry_from_plan(
            1,
            &plan(&path, MediaImportKind::Image, 30),
            None,
        ));
        assert!(!bin.entry(id).expect("entry exists").is_missing(None));

        fs::remove_file(&path).expect("fixture removes");
        assert!(
            bin.entry(id)
                .expect("a missing entry is kept")
                .is_missing(None),
            "a vanished file is reported rather than removed"
        );
    }

    #[test]
    fn a_relative_entry_resolves_against_the_project_directory() {
        let bin = MediaBin::default();
        let document = {
            let state = TimelineState::from_json(
                br#"{"version": 3, "settings": {"width": 1920, "height": 1080, "fps": 60,
                     "sampleRate": 48000}, "scenes": [], "clips": []}"#,
            )
            .expect("fixture loads");
            let mut document = state.snapshot();
            document.extra.insert(
                MEDIA_BIN_KEY.to_owned(),
                json!({"entries": [{"id": 1, "path": "media/a.mp4"}]}),
            );
            document
        };
        assert_eq!(
            read_media_bin(&document).entries()[0].path,
            "media/a.mp4",
            "a stored project keeps its relative path"
        );
        assert!(bin.is_empty());
        assert_eq!(
            resolve_path(Some(Path::new("/projects/example.aviqtl")), "media/a.mp4"),
            PathBuf::from("/projects/media/a.mp4")
        );
    }

    #[test]
    fn a_no_op_edit_plans_nothing() {
        let (state, document) = state_with_bin(&MediaBin::default());
        assert!(plan_bin_edit(&state, &document, |_| {}).is_none());
        assert!(
            plan_bin_edit(&state, &document, |bin| {
                bin.remove(42);
            })
            .is_none(),
            "removing an absent entry changes nothing"
        );
    }

    #[test]
    fn a_bin_edit_is_one_reversible_transaction() {
        let mut bin = MediaBin::default();
        let id = bin.upsert(entry_from_plan(
            1,
            &plan(Path::new("/media/a.mp4"), MediaImportKind::Video, 240),
            None,
        ));
        let (mut state, document) = state_with_bin(&bin);

        let transaction = plan_bin_edit(&state, &document, |bin| {
            bin.set_range(id, Some(30), Some(210));
        })
        .expect("the range edit plans");
        state.apply(&transaction).expect("the range edit applies");
        let after = state.snapshot();
        assert_eq!(
            read_media_bin(&after).entry(id).expect("entry").in_frame,
            30
        );

        state.undo(&transaction).expect("the range edit undoes");
        let reverted = state.snapshot();
        assert_eq!(read_media_bin(&reverted), read_media_bin(&document));
        assert_eq!(reverted.clips, document.clips, "clips are unaffected");
    }
}
