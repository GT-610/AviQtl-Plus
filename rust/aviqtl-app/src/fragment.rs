//! AviQtl-Plus fragment files: a block of clips saved on its own.
//!
//! A fragment is what a user reaches for when work has to leave the project it was made in: a
//! title card, an effect chain, a whole scene's worth of layered objects. AviUtl solves this
//! with the object file (`.exo`), and AviQtl-Plus reads those, but writing one is a lossy trip
//! through another program's vocabulary — anything AviQtl-Plus can express and ExEdit cannot
//! has to be dropped, and the file says so.
//!
//! A fragment keeps the clips exactly as this build stores them: the same effect stack, the
//! same keyframes, the same parameters, including the ones no AviUtl filter has. Nothing is
//! translated, so nothing is lost, and no AviUtl semantics are guessed at. That makes it the
//! format to save in, with the object file reserved for handing work to someone still on
//! AviUtl.
//!
//! The file is JSON: the project's own clip representation, plus the settings the clips were
//! authored against so an import can say when the frame rate or frame size differs.

use aviqtl_rust_core::api::{ClipDocument, ProjectDocument, ProjectSettings, SceneDocument};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Marker that tells a fragment apart from a project document, which is also JSON.
pub const FRAGMENT_FORMAT: &str = "aviqtl.fragment";

/// Fragment layout version. A reader rejects anything it does not know how to interpret
/// rather than guessing, because the clips it would produce would be silently wrong.
pub const FRAGMENT_VERSION: i32 = 1;

/// File extension a fragment is saved with.
pub const FRAGMENT_EXTENSION: &str = "aviqtlfrag";

/// Largest fragment this build will read. A fragment carries clips, not media, so this is
/// generous; the cap keeps a malformed or hostile file from allocating without bound.
pub const MAX_FRAGMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Clip count beyond which a file is treated as malformed rather than as a big fragment.
pub const MAX_FRAGMENT_CLIPS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragmentDocument {
    /// Always [`FRAGMENT_FORMAT`]; checked on read.
    pub format: String,
    pub version: i32,
    /// The settings the clips were authored against.
    pub settings: ProjectSettings,
    pub clips: Vec<ClipDocument>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentError {
    /// The file is JSON but not a fragment (usually a project document).
    NotAFragment,
    UnsupportedVersion,
    TooManyClips,
    InvalidDocument,
    Serialization,
}

impl Display for FragmentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotAFragment => "the file is not an AviQtl-Plus fragment",
            Self::UnsupportedVersion => "the fragment was written by a newer AviQtl-Plus",
            Self::TooManyClips => "the fragment holds too many clips",
            Self::InvalidDocument => "the fragment is damaged or incomplete",
            Self::Serialization => "the fragment could not be serialized",
        })
    }
}

impl Error for FragmentError {}

impl FragmentDocument {
    /// Collects `clips` into a fragment authored against `scene`.
    ///
    /// The scene's resolution and frame rate ride along so a later import can tell the user
    /// when the destination disagrees; the audio rate comes from the project, which is the
    /// only place that holds it.
    pub fn new(
        document: &ProjectDocument,
        scene: &SceneDocument,
        clips: impl IntoIterator<Item = ClipDocument>,
    ) -> Self {
        Self {
            format: FRAGMENT_FORMAT.to_owned(),
            version: FRAGMENT_VERSION,
            settings: ProjectSettings {
                width: scene.width,
                height: scene.height,
                fps: scene.fps,
                sample_rate: document.settings.sample_rate,
                extra: Default::default(),
            },
            clips: clips.into_iter().collect(),
            extra: BTreeMap::new(),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, FragmentError> {
        // Read the marker and version before deserializing the clips: a project document has
        // no `format` field, and saying so is more useful than a field-by-field parse error.
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| FragmentError::InvalidDocument)?;
        let object = value.as_object().ok_or(FragmentError::InvalidDocument)?;
        match object.get("format").and_then(serde_json::Value::as_str) {
            Some(FRAGMENT_FORMAT) => {}
            Some(_) | None => return Err(FragmentError::NotAFragment),
        }
        let version = object
            .get("version")
            .and_then(serde_json::Value::as_i64)
            .ok_or(FragmentError::InvalidDocument)?;
        if version != i64::from(FRAGMENT_VERSION) {
            return Err(FragmentError::UnsupportedVersion);
        }
        let clip_count = object
            .get("clips")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .ok_or(FragmentError::InvalidDocument)?;
        if clip_count > MAX_FRAGMENT_CLIPS {
            return Err(FragmentError::TooManyClips);
        }
        serde_json::from_value(value).map_err(|_| FragmentError::InvalidDocument)
    }

    /// Pretty JSON: a fragment is small, and a diffable one is easier to reason about than a
    /// dense single line.
    pub fn to_bytes(&self) -> Result<Vec<u8>, FragmentError> {
        serde_json::to_vec_pretty(self).map_err(|_| FragmentError::Serialization)
    }

    /// The first frame and lowest layer the fragment occupies.
    ///
    /// A fragment is inserted relative to where its own content starts, so that dropping it
    /// at the playhead puts its top-left corner there instead of scattering it around frame
    /// zero.
    pub fn anchor(&self) -> (i32, i32) {
        let frame = self.clips.iter().map(|clip| clip.start).min().unwrap_or(0);
        let layer = self.clips.iter().map(|clip| clip.layer).min().unwrap_or(0);
        (frame.max(0), layer.max(0))
    }
}

/// Reads a fragment from disk.
pub fn load_fragment(path: &std::path::Path) -> Result<FragmentDocument, String> {
    let bytes = crate::file_io::read_bounded(path, MAX_FRAGMENT_BYTES)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    FragmentDocument::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// Writes a fragment, replacing any existing file atomically.
pub fn save_fragment(path: &std::path::Path, fragment: &FragmentDocument) -> Result<(), String> {
    let bytes = fragment
        .to_bytes()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    crate::project_io::write_atomic(path, &bytes)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Where the fragment's clips land.
#[derive(Debug, Clone, Copy)]
pub struct FragmentPlacement {
    /// Added to every clip's start frame.
    pub frame_offset: i32,
    /// Added to every clip's layer.
    pub layer_offset: i32,
    /// Upper bound on layers in the destination scene.
    pub maximum_layers: i32,
}

impl Default for FragmentPlacement {
    fn default() -> Self {
        Self {
            frame_offset: 0,
            layer_offset: 0,
            maximum_layers: 512,
        }
    }
}

/// What an import did, for the status line and the dialog shown when it needs explaining.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FragmentReport {
    pub clips: usize,
    pub selection_only: bool,
    pub notes: Vec<String>,
}

impl FragmentReport {
    pub fn summary(&self) -> String {
        let mut summary = format!("Imported {} clip(s) from a fragment", self.clips);
        if !self.notes.is_empty() {
            summary.push_str(&format!(", {} note(s)", self.notes.len()));
        }
        summary
    }

    pub fn details(&self, max_notes: usize) -> String {
        let mut lines = vec![self.summary()];
        if self.notes.is_empty() {
            return lines.join("\n");
        }
        lines.push(String::new());
        lines.extend(self.notes.iter().take(max_notes).cloned());
        if self.notes.len() > max_notes {
            lines.push(format!(
                "… and {} more",
                self.notes.len().saturating_sub(max_notes)
            ));
        }
        lines.join("\n")
    }
}

/// Plans the clips a fragment inserts: new ids, the requested offset, and a clear run of
/// frames. Nothing is applied here, so the workspace can run the import as one transaction.
pub fn plan_import(
    fragment: &FragmentDocument,
    scene: &SceneDocument,
    document: &ProjectDocument,
    first_clip_id: i32,
    placement: FragmentPlacement,
) -> (Vec<ClipDocument>, FragmentReport) {
    let maximum_layer = placement.maximum_layers.clamp(1, 512) - 1;
    let mut report = FragmentReport::default();
    if fragment.settings.fps != scene.fps && fragment.settings.fps > 0.0 {
        // Frames are the fragment's unit of time, so a different rate stretches every clip.
        report.notes.push(format!(
            "the fragment was made at {} fps and this scene runs at {} fps, so its timing differs",
            fragment.settings.fps, scene.fps
        ));
    }
    if (fragment.settings.width, fragment.settings.height) != (scene.width, scene.height) {
        report.notes.push(format!(
            "the fragment was made for {}x{} and this scene is {}x{}",
            fragment.settings.width, fragment.settings.height, scene.width, scene.height
        ));
    }
    let mut clips = fragment
        .clips
        .iter()
        .enumerate()
        .map(|(index, clip)| {
            let mut clip = clip.clone();
            clip.id = first_clip_id.saturating_add(index as i32);
            clip.scene_id = scene.id;
            clip.start = clip.start.saturating_add(placement.frame_offset).max(0);
            clip.layer = clip
                .layer
                .saturating_add(placement.layer_offset)
                .clamp(0, maximum_layer);
            clip
        })
        .collect::<Vec<_>>();
    let shift = crate::clip_insert::shift_past_existing(document, scene.id, &mut clips);
    if shift > 0 {
        report.notes.push(crate::clip_insert::shift_note(shift));
    }
    report.clips = clips.len();
    (clips, report)
}

/// Picks the clips a fragment export writes.
///
/// `ids` is the current selection. When it is empty the whole scene is exported, which is what
/// a user pressing export with nothing selected means.
pub fn select_clips<'a>(
    document: &'a ProjectDocument,
    scene_id: i32,
    ids: &[i32],
) -> (Vec<&'a ClipDocument>, bool) {
    let mut clips = document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene_id && (ids.is_empty() || ids.contains(&clip.id)))
        .collect::<Vec<_>>();
    // Timeline order, so the file reads the way the timeline does.
    clips.sort_by_key(|clip| (clip.layer, clip.start, clip.id));
    (clips, !ids.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};

    fn clip(id: i32, start: i32, duration: i32, layer: i32) -> ClipDocument {
        ClipDocument {
            id,
            scene_id: 1,
            clip_type: "text".to_owned(),
            start,
            duration,
            layer,
            clip_by_upper_object: false,
            params: Map::new(),
            audio_plugins: Vec::new(),
            effects: Vec::new(),
            extra: Default::default(),
        }
    }

    fn scene() -> SceneDocument {
        SceneDocument {
            id: 1,
            name: "Root".to_owned(),
            width: 1920,
            height: 1080,
            fps: 60.0,
            start: 0,
            duration: 600,
            nested_duration: 0,
            locked_layers: Vec::new(),
            hidden_layers: Vec::new(),
            grid_mode: "none".to_owned(),
            grid_bpm: 120.0,
            grid_offset: 0.0,
            grid_interval: 10,
            grid_subdivision: 4,
            enable_snap: true,
            magnetic_snap_range: 10,
            extra: Default::default(),
        }
    }

    fn project(clips: Vec<ClipDocument>) -> ProjectDocument {
        ProjectDocument {
            version: 3,
            settings: ProjectSettings {
                width: 1920,
                height: 1080,
                fps: 60.0,
                sample_rate: 48_000,
                extra: Default::default(),
            },
            scenes: vec![scene()],
            clips,
            extra: Default::default(),
        }
    }

    #[test]
    fn a_fragment_round_trips_through_its_bytes() {
        let document = project(vec![clip(1, 0, 30, 0)]);
        let fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        let bytes = fragment.to_bytes().expect("serializes");
        let reparsed = FragmentDocument::parse(&bytes).expect("parses");
        assert_eq!(reparsed, fragment);
    }

    /// The documented file shape, pinned: the developer page shows this JSON, and a reader
    /// that relied on it would break silently if a field were renamed.
    #[test]
    fn the_file_carries_the_marker_and_the_authoring_settings() {
        let document = project(vec![clip(1, 0, 30, 0)]);
        let fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        let json: serde_json::Value =
            serde_json::from_slice(&fragment.to_bytes().expect("serializes")).expect("json");
        assert_eq!(json["format"], Value::from(FRAGMENT_FORMAT));
        assert_eq!(json["version"], Value::from(FRAGMENT_VERSION));
        assert_eq!(json["settings"]["fps"], Value::from(60.0));
        assert_eq!(json["settings"]["sampleRate"], Value::from(48_000));
        let clip = &json["clips"][0];
        assert_eq!(clip["type"], Value::from("text"));
        assert_eq!(clip["sceneId"], Value::from(1));
        assert!(clip["effects"].is_array(), "{clip}");
        assert!(clip["params"].is_object(), "{clip}");
    }

    #[test]
    fn a_project_document_is_rejected_with_a_useful_error() {
        let document = project(vec![clip(1, 0, 30, 0)]);
        let bytes = serde_json::to_vec(&document).expect("project serializes");
        assert_eq!(
            FragmentDocument::parse(&bytes),
            Err(FragmentError::NotAFragment)
        );
    }

    #[test]
    fn an_unknown_version_is_rejected_rather_than_guessed_at() {
        let document = project(vec![clip(1, 0, 30, 0)]);
        let mut fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        fragment.version = FRAGMENT_VERSION + 1;
        let bytes = fragment.to_bytes().expect("serializes");
        assert_eq!(
            FragmentDocument::parse(&bytes),
            Err(FragmentError::UnsupportedVersion)
        );
    }

    #[test]
    fn damaged_json_is_reported_as_damaged() {
        assert_eq!(
            FragmentDocument::parse(b"{not json"),
            Err(FragmentError::InvalidDocument)
        );
        assert_eq!(
            FragmentDocument::parse(br#"{"format":"aviqtl.fragment","version":1}"#),
            Err(FragmentError::InvalidDocument)
        );
    }

    #[test]
    fn the_anchor_is_the_first_frame_and_lowest_layer() {
        let document = project(vec![
            clip(1, 40, 10, 3),
            clip(2, 10, 10, 7),
            clip(3, 25, 10, 1),
        ]);
        let fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        assert_eq!(fragment.anchor(), (10, 1));
    }

    #[test]
    fn an_import_renumbers_the_clips_and_applies_the_offset() {
        let document = project(vec![clip(1, 10, 10, 2)]);
        let fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        let placement = FragmentPlacement {
            frame_offset: 5,
            layer_offset: 1,
            maximum_layers: 8,
        };
        let (clips, report) =
            plan_import(&fragment, &scene(), &project(Vec::new()), 100, placement);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, 100);
        assert_eq!(clips[0].start, 15);
        assert_eq!(clips[0].layer, 3);
        assert_eq!(report.clips, 1);
    }

    #[test]
    fn a_placement_beyond_the_layer_limit_is_clamped() {
        let document = project(vec![clip(1, 0, 10, 6)]);
        let fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        let placement = FragmentPlacement {
            frame_offset: 0,
            layer_offset: 3,
            maximum_layers: 4,
        };
        let (clips, _) = plan_import(&fragment, &scene(), &project(Vec::new()), 100, placement);
        assert_eq!(clips[0].layer, 3, "the last layer of a four-layer timeline");
    }

    #[test]
    fn a_fragment_onto_occupied_frames_moves_forward() {
        let fragment =
            FragmentDocument::new(&project(Vec::new()), &scene(), vec![clip(1, 0, 10, 0)]);
        let occupied = project(vec![clip(9, 0, 30, 0)]);
        let (clips, report) = plan_import(
            &fragment,
            &scene(),
            &occupied,
            100,
            FragmentPlacement::default(),
        );
        assert_eq!(clips[0].start, 30);
        assert!(
            report.notes.iter().any(|note| note.contains("moved")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn a_different_frame_rate_is_reported() {
        let mut document = project(Vec::new());
        document.settings.fps = 24.0;
        let mut source = scene();
        source.fps = 24.0;
        let fragment = FragmentDocument::new(&document, &source, vec![clip(1, 0, 10, 0)]);
        let (_, report) = plan_import(
            &fragment,
            &scene(),
            &project(Vec::new()),
            100,
            FragmentPlacement::default(),
        );
        assert!(
            report.notes.iter().any(|note| note.contains("24")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn an_empty_selection_means_the_whole_scene() {
        let document = project(vec![
            clip(1, 10, 10, 1),
            clip(2, 0, 10, 0),
            clip(3, 0, 10, 0),
        ]);
        let (all, selection_only) = select_clips(&document, 1, &[]);
        assert_eq!(all.len(), 3);
        assert!(!selection_only);
        // Timeline order: layer first, then frame.
        assert_eq!(
            all.iter().map(|clip| clip.id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );

        let (some, selection_only) = select_clips(&document, 1, &[3]);
        assert_eq!(some.len(), 1);
        assert_eq!(some[0].id, 3);
        assert!(selection_only);

        // A clip from another scene never leaks into the export.
        let mut other = document.clone();
        other.clips[0].scene_id = 2;
        let (scoped, _) = select_clips(&other, 1, &[]);
        assert_eq!(scoped.len(), 2);
    }

    #[test]
    fn a_fragment_survives_a_trip_through_the_filesystem() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("title.aviqtlfrag");
        let document = project(vec![clip(1, 0, 30, 0), clip(2, 5, 25, 1)]);
        let fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        save_fragment(&path, &fragment).expect("saves");
        assert_eq!(load_fragment(&path).expect("loads"), fragment);
    }

    #[test]
    fn loading_a_missing_file_names_the_path() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("absent.aviqtlfrag");
        let message = load_fragment(&path).expect_err("the file does not exist");
        assert!(message.contains("absent.aviqtlfrag"), "{message}");
    }

    #[test]
    fn a_file_that_is_not_json_is_reported_as_damaged() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("broken.aviqtlfrag");
        std::fs::write(&path, b"not a fragment").expect("writes");
        let message = load_fragment(&path).expect_err("the file is not a fragment");
        assert!(message.contains("damaged"), "{message}");
    }

    #[test]
    fn a_clip_keeps_parameters_no_aviutl_filter_has() {
        // The reason this format exists: an AviUtl object file has nowhere to put an
        // AviQtl-Plus-only parameter, and a fragment does.
        let mut clip = clip(1, 0, 10, 0);
        clip.params
            .insert("aviqtlOnly".to_owned(), Value::from("kept"));
        let document = project(vec![clip]);
        let fragment = FragmentDocument::new(&document, &scene(), document.clips.clone());
        let reparsed =
            FragmentDocument::parse(&fragment.to_bytes().expect("serializes")).expect("parses");
        assert_eq!(reparsed.clips[0].params["aviqtlOnly"], Value::from("kept"));
    }
}
