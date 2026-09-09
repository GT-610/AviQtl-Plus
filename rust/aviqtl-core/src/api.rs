//! Safe Rust entry points for application code.
//!
//! This module deliberately keeps the project document and reversible patch representation
//! private. Rust front ends can own and edit the authoritative timeline without crossing the C
//! ABI, while the existing Qt adapter continues to use the unchanged exported ABI.

use crate::abi::{AviQtlSceneSettings, AviQtlTimelineClipGeometry};
use crate::project::parse_project_document;
use crate::timeline_state::{
    EntityKind, StateError, TimelineState as CoreTimelineState, Transaction,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

pub use crate::audio::{
    AudioProcessError, StereoMix, StereoMixParameters, StereoMixTrack, StereoTrackMeter,
    mix_stereo_tracks, resample_stereo,
};
pub use crate::bake_plan::{
    AudioLayerPlan, CameraRenderPlan, EvaluatedAudioPlugin, EvaluatedEffect, EvaluatedTransform,
    LensFlareRenderPlan, MediaKind, MediaPlaybackPlan, NestedSceneRenderPlan,
    ParticleFieldRenderPlan, ProceduralObjectRenderPlan, RadialLinesRenderPlan, RenderLayerPlan,
    RgbaColor, SceneFramePlan, SceneRenderPlan, SceneRenderPlanError, ShapeGradientKind, ShapeKind,
    ShapeRenderPlan, TextAlignment, TextRenderPlan, TrackLineRenderPlan,
};
pub use crate::export::{
    ExportAudioFramePlan, ExportCodecBackend, ExportConfigurationError, ExportFixedGopMode,
    ExportImageFormat, ExportProgressPlan, ImageSequenceExportPlan, ImageSequenceExportRequest,
    VideoExportDefaults, VideoExportPlan, VideoExportRequest, export_codec_backend,
    export_codec_fallback, export_encoder_queue_size, export_fixed_gop_mode,
    plan_export_audio_frame, plan_export_progress, plan_image_sequence_export, plan_video_export,
    video_export_defaults,
};
pub use crate::permission::{PluginPermission, PluginPermissionState};
pub use crate::plugin::{
    AudioPluginInfo, ScriptPluginIdentity, ScriptPluginManifest, ScriptPluginValidationStatus,
    audio_plugin_categories, audio_plugins_in_category, deduplicate_audio_plugins,
    normalize_audio_plugin_category, parse_audio_plugin_discovery_output,
    parse_script_plugin_manifest, validate_script_plugin_manifest,
};
pub use crate::preset::{
    EffectPreset, PresetError, build_effect_preset, parse_effect_preset, preset_name_is_safe,
};
pub use crate::project::{
    AudioPluginDocument, ClipDocument, EffectDocument, ExtraFields, MAX_TIMELINE_LAYER,
    MAX_TIMELINE_LAYERS, ProjectDocument, ProjectSettings, SceneDocument,
};
pub use crate::recovery::{
    RecoveryInspection, RecoveryMetadata, build_recovery_metadata, inspect_recovery_metadata,
    recovery_id_from_snapshot_name, recovery_id_is_valid, recovery_snapshot_name_is_valid,
};
pub use crate::script::{
    ScriptMetadata, ScriptParameter, ScriptParameterGroup, ScriptParameterKind,
    ScriptParameterOption, inspect_script_metadata,
};
pub use crate::script_runtime::{
    ScriptClipSnapshot, ScriptExecution, ScriptHook, ScriptHostCommand, ScriptHostSnapshot,
    ScriptRuntime, ScriptRuntimeError,
};
pub use crate::settings::{SettingsError, SettingsMutation, SettingsState};
pub use crate::timeline_state::{
    ClipGeometryUpdate, ClipReplacement, EditRequest as TimelineCommand, EffectInsertion,
};

/// Returns the visible timeline duration for the supplied clips.
///
/// Non-positive clip durations are ignored and an empty timeline is one frame long, matching the
/// Qt timeline controller and the C ABI entry point.
pub fn timeline_duration<'a>(clips: impl IntoIterator<Item = &'a ClipDocument>) -> i32 {
    let geometry = clips
        .into_iter()
        .map(|clip| AviQtlTimelineClipGeometry {
            clip_id: clip.id,
            layer: clip.layer,
            start_frame: clip.start,
            duration_frames: clip.duration,
        })
        .collect::<Vec<_>>();
    crate::timeline_domain::timeline_duration(&geometry)
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyframePoint {
    pub frame: i32,
    pub value: Value,
    pub interpolation: String,
    pub options: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub categories: Vec<String>,
    pub params: serde_json::Map<String, Value>,
    pub ui: serde_json::Map<String, Value>,
    pub source: String,
    pub package_id: String,
    pub source_path: String,
}

/// Rust-owned package catalog projection shared by native front ends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageCatalog {
    catalog: Vec<serde_json::Map<String, Value>>,
}

impl PackageCatalog {
    /// Removes every repository package while retaining the catalog allocation.
    pub fn clear(&mut self) {
        self.catalog.clear();
    }

    /// Merges one repository catalog using AviQtl's version and priority rules.
    pub fn merge(
        &mut self,
        packages: &[Value],
        repository: &serde_json::Map<String, Value>,
        repositories: &[Value],
        installed: &serde_json::Map<String, Value>,
        language: &str,
        app_version: &str,
    ) {
        let packages = packages
            .iter()
            .filter_map(Value::as_object)
            .cloned()
            .collect::<Vec<_>>();
        let repositories = repositories
            .iter()
            .filter_map(Value::as_object)
            .cloned()
            .collect::<Vec<_>>();
        self.catalog = crate::package::merge_catalog_packages(
            std::mem::take(&mut self.catalog),
            &packages,
            repository,
            &repositories,
            installed,
            language,
            app_version,
        );
    }

    /// Returns one Qt-compatible package category or the installed projection.
    pub fn packages_by_type(&self, package_type: &str) -> Vec<Value> {
        crate::package::filter_catalog(&self.catalog, package_type)
    }

    /// Finds a package, optionally preferring one source repository.
    pub fn find(
        &self,
        package_id: &str,
        source_repository: &str,
    ) -> Option<serde_json::Map<String, Value>> {
        crate::package::find_package(&self.catalog, package_id, source_repository)
            .as_object()
            .cloned()
    }

    /// Reports whether any installed package has a newer catalog version.
    pub fn has_updates(&self) -> bool {
        crate::package::has_updates(&self.catalog)
    }

    /// Lists package IDs eligible for the Qt upgrade-all flow.
    pub fn upgrade_ids(&self) -> Vec<String> {
        crate::package::upgrade_ids(&self.catalog)
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect()
    }

    /// Updates the installed-version projection after a deployment transaction.
    pub fn set_installed(&mut self, package_id: &str, version: Option<&str>) -> bool {
        crate::package::set_installed(&mut self.catalog, package_id, version)
    }
}

/// Mutation requested for AviQtl's configured package repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageRepositoryOperation {
    Add { enabled: bool, priority: i32 },
    Remove,
    SetEnabled(bool),
    SetPriority(i32),
}

/// Applies the same repository normalization and duplicate policy as the Qt adapter.
pub fn mutate_package_repositories(
    repositories: &[Value],
    url: &str,
    operation: PackageRepositoryOperation,
) -> Option<(Vec<Value>, bool)> {
    let repositories = repositories
        .iter()
        .filter_map(Value::as_object)
        .cloned()
        .collect::<Vec<_>>();
    let (name, enabled, priority) = match operation {
        PackageRepositoryOperation::Add { enabled, priority } => ("add", enabled, priority),
        PackageRepositoryOperation::Remove => ("remove", true, 10),
        PackageRepositoryOperation::SetEnabled(enabled) => ("enabled", enabled, 10),
        PackageRepositoryOperation::SetPriority(priority) => ("priority", true, priority),
    };
    crate::package::mutate_repositories(repositories, name, url, enabled, priority)
}

/// Returns enabled repositories sorted by configured priority.
pub fn enabled_package_repositories(repositories: &[Value]) -> Vec<Value> {
    crate::package::enabled_repositories(
        repositories
            .iter()
            .filter_map(Value::as_object)
            .cloned()
            .collect(),
    )
}

/// Validates package-detail metadata before it is cached or used for installation.
pub fn normalize_package_metadata(
    detail: &serde_json::Map<String, Value>,
) -> Option<serde_json::Map<String, Value>> {
    crate::package::normalize_metadata(detail)
}

/// Validated package archive selection returned by package-detail metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInstallSelection {
    pub status: String,
    pub version: String,
    pub download_url: String,
    pub sha256: String,
    pub minimum_app_version: String,
    pub package_type: String,
}

/// Selects the requested or latest package version using the Qt package-manager policy.
pub fn select_package_install(
    detail: &serde_json::Map<String, Value>,
    requested_version: &str,
    app_version: &str,
) -> PackageInstallSelection {
    let selection = crate::package::select_install(detail, requested_version, app_version);
    let string = |name: &str| {
        selection
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    PackageInstallSelection {
        status: string("status"),
        version: string("version"),
        download_url: string("downloadUrl"),
        sha256: string("sha256"),
        minimum_app_version: string("minAppVersion"),
        package_type: string("type"),
    }
}

/// Validates package identifiers before they are used as filesystem names.
pub fn package_id_is_valid(package_id: &str) -> bool {
    crate::policy::valid_package_id(package_id)
}

/// Reports whether a package type has an installable filesystem destination.
pub fn package_type_is_installable(package_type: &str) -> bool {
    matches!(package_type, "mod" | "effect" | "object")
}

/// Validates one ZIP entry path using AviQtl's traversal policy.
pub fn package_archive_path_is_safe(path: &str) -> bool {
    crate::policy::safe_archive_path(path)
}

/// Validates and normalizes one effect or object metadata document for Rust front ends.
pub fn parse_effect_metadata(input: &[u8]) -> Option<EffectMetadata> {
    let metadata = crate::effect::normalize_metadata(input)?;
    let string = |name: &str| {
        metadata
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Some(EffectMetadata {
        id: string("id"),
        name: string("name"),
        version: string("version"),
        kind: string("kind"),
        categories: metadata
            .get("categories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        params: metadata
            .get("params")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        ui: metadata
            .get("ui")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        source: string("source"),
        package_id: string("packageId"),
        source_path: string("sourcePath"),
    })
}

/// Returns normalized keyframe points without exposing the persisted JSON representation.
pub fn inspect_keyframe_track(
    track: Option<&Value>,
    fallback: &Value,
    duration: i32,
) -> Vec<KeyframePoint> {
    let Some(track) = track else {
        return Vec::new();
    };
    crate::keyframe_document::inspect_track(track, fallback, duration)
        .into_iter()
        .map(|point| KeyframePoint {
            frame: point.frame,
            value: point.value,
            interpolation: point.interpolation,
            options: point.options,
        })
        .collect()
}

/// Evaluates a persisted keyframe track at a clip-relative frame.
pub fn evaluate_keyframe_track(
    track: Option<&Value>,
    fallback: &Value,
    duration: i32,
    frame: i32,
) -> Value {
    track.map_or_else(
        || fallback.clone(),
        |track| {
            crate::keyframe_document::evaluate_document_track(
                track,
                fallback,
                duration,
                frame.clamp(0, duration.max(0)),
            )
        },
    )
}

/// Lists every interpolation identifier accepted by the Rust keyframe evaluator.
pub fn keyframe_interpolation_names() -> Vec<&'static str> {
    let mut names = Vec::with_capacity(crate::EasingKind::NAMES.len() + 3);
    names.push("none");
    names.extend(crate::EasingKind::NAMES);
    names.extend(["random", "alternate"]);
    names
}

/// Applies the Rust-owned grid policy to a scene-relative timeline frame.
pub fn snap_scene_frame(
    frame: f64,
    ignore_snap: bool,
    scene: &SceneDocument,
    timeline_scale: f64,
) -> i32 {
    let grid_mode = match scene.grid_mode.as_str() {
        "BPM" => 1,
        "Frame" => 2,
        _ => 0,
    };
    crate::timeline_domain::snap_frame(
        frame,
        ignore_snap,
        AviQtlSceneSettings {
            width: scene.width,
            height: scene.height,
            fps: scene.fps,
            total_frames: scene.duration,
            grid_mode,
            grid_bpm: scene.grid_bpm,
            grid_offset: scene.grid_offset,
            grid_interval: scene.grid_interval,
            grid_subdivision: scene.grid_subdivision,
            enable_snap: u32::from(scene.enable_snap),
            magnetic_snap_range: scene.magnetic_snap_range,
        },
        timeline_scale,
    )
}

/// Plans a collision-resolved delta move using the same policy as the Qt timeline adapter.
pub fn plan_clip_delta_move(
    document: &ProjectDocument,
    scene_id: i32,
    moving_ids: &[i32],
    delta_layer: i32,
    delta_frame: i32,
) -> Result<Vec<ClipGeometryUpdate>, TimelineError> {
    let scene = document
        .scenes
        .iter()
        .find(|scene| scene.id == scene_id)
        .ok_or(TimelineError::InvalidArgument)?;
    let geometry = scene_clip_geometry(document, scene_id);
    let unique_ids = moving_ids.iter().copied().collect::<BTreeSet<_>>();
    if unique_ids.is_empty()
        || unique_ids
            .iter()
            .any(|clip_id| !geometry.iter().any(|clip| clip.clip_id == *clip_id))
    {
        return Err(TimelineError::InvalidArgument);
    }
    crate::timeline_edit::plan_delta_move(
        &geometry,
        &unique_ids.iter().copied().collect::<Vec<_>>(),
        &scene.locked_layers,
        delta_layer,
        delta_frame,
    )
    .map(clip_geometry_updates)
    .map_err(|_| TimelineError::InvalidArgument)
}

/// Plans one or more timeline trims while preserving positive durations and locked layers.
pub fn plan_clip_resize(
    document: &ProjectDocument,
    scene_id: i32,
    clip_ids: &[i32],
    delta_start_frame: i32,
    delta_duration_frames: i32,
) -> Result<Vec<ClipGeometryUpdate>, TimelineError> {
    let scene = document
        .scenes
        .iter()
        .find(|scene| scene.id == scene_id)
        .ok_or(TimelineError::InvalidArgument)?;
    let geometry = scene_clip_geometry(document, scene_id);
    let unique_ids = clip_ids.iter().copied().collect::<BTreeSet<_>>();
    if unique_ids.is_empty() {
        return Err(TimelineError::InvalidArgument);
    }
    let selected = unique_ids
        .iter()
        .map(|clip_id| {
            geometry
                .iter()
                .find(|clip| clip.clip_id == *clip_id)
                .copied()
                .ok_or(TimelineError::InvalidArgument)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if selected
        .iter()
        .any(|clip| scene.locked_layers.contains(&clip.layer))
    {
        return Err(TimelineError::InvalidArgument);
    }
    Ok(clip_geometry_updates(crate::timeline_edit::plan_resize(
        &selected,
        delta_start_frame,
        delta_duration_frames,
    )))
}

/// Builds the stable new-index-to-old-index permutation used by the Qt effect sidebar.
pub fn plan_effect_reorder(
    length: usize,
    selected_indices: &[usize],
    target_index: usize,
    minimum_index: usize,
) -> Result<Vec<usize>, TimelineError> {
    let selected = selected_indices
        .iter()
        .copied()
        .map(i32::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TimelineError::InvalidArgument)?;
    let target_index = i32::try_from(target_index).map_err(|_| TimelineError::InvalidArgument)?;
    let minimum_index = i32::try_from(minimum_index).map_err(|_| TimelineError::InvalidArgument)?;
    let permutation = if selected.len() == 1 {
        crate::timeline_domain::plan_index_move(length, selected[0], target_index, minimum_index)
            .map(|(permutation, _)| permutation)
    } else {
        crate::timeline_domain::plan_multi_reorder(length, &selected, target_index, minimum_index)
            .map(|(permutation, _, _)| permutation)
    }
    .ok_or(TimelineError::InvalidArgument)?;
    permutation
        .into_iter()
        .map(|index| usize::try_from(index).map_err(|_| TimelineError::InvalidArgument))
        .collect()
}

/// Plans insertion of empty timeline layers above or below a target layer.
pub fn plan_scene_layer_insertion(
    document: &ProjectDocument,
    scene_id: i32,
    target_layer: i32,
    count: i32,
    above: bool,
) -> Result<Vec<ClipGeometryUpdate>, TimelineError> {
    if !document.scenes.iter().any(|scene| scene.id == scene_id) {
        return Err(TimelineError::InvalidArgument);
    }
    crate::timeline_edit::plan_insert_layers(
        &scene_clip_geometry(document, scene_id),
        target_layer,
        count,
        above,
    )
    .map(clip_geometry_updates)
    .map_err(|_| TimelineError::InvalidArgument)
}

/// Plans a layer-range shift with the same bounds and collision rules as the Qt timeline.
pub fn plan_scene_layer_shift(
    document: &ProjectDocument,
    scene_id: i32,
    start_layer: i32,
    end_layer: i32,
    delta: i32,
) -> Result<Vec<ClipGeometryUpdate>, TimelineError> {
    if !document.scenes.iter().any(|scene| scene.id == scene_id) {
        return Err(TimelineError::InvalidArgument);
    }
    crate::timeline_edit::plan_shift_layers(
        &scene_clip_geometry(document, scene_id),
        start_layer,
        end_layer,
        delta,
    )
    .map(clip_geometry_updates)
    .map_err(|_| TimelineError::InvalidArgument)
}

/// Returns the frame span preserved by a timeline clipboard operation.
pub fn clipboard_duration(clips: &[ClipDocument]) -> i32 {
    crate::timeline_edit::clipboard_duration(&clip_document_geometry(clips))
}

/// Plans a collision-free clipboard paste while preserving relative frame and layer offsets.
pub fn plan_clipboard_paste(
    document: &ProjectDocument,
    scene_id: i32,
    clipboard: &[ClipDocument],
    requested_frame: i32,
    requested_layer: i32,
) -> Result<(i32, Vec<ClipGeometryUpdate>), TimelineError> {
    if !document.scenes.iter().any(|scene| scene.id == scene_id) {
        return Err(TimelineError::InvalidArgument);
    }
    let existing = scene_clip_geometry(document, scene_id);
    let clipboard = clip_document_geometry(clipboard);
    crate::timeline_edit::plan_clipboard_placement(
        &existing,
        &clipboard,
        requested_frame,
        requested_layer,
    )
    .map(|(safe_frame, geometry)| (safe_frame, clip_geometry_updates(geometry)))
    .map_err(|_| TimelineError::InvalidArgument)
}

/// Finds the first collision-free frame on one scene layer.
pub fn find_vacant_scene_frame(
    document: &ProjectDocument,
    scene_id: i32,
    excluded_ids: &[i32],
    layer: i32,
    start_frame: i32,
    duration_frames: i32,
) -> Result<i32, TimelineError> {
    if !(0..=MAX_TIMELINE_LAYER).contains(&layer) || duration_frames <= 0 {
        return Err(TimelineError::InvalidArgument);
    }
    if !document.scenes.iter().any(|scene| scene.id == scene_id) {
        return Err(TimelineError::InvalidArgument);
    }
    Ok(crate::timeline_edit::find_vacant_frame(
        &scene_clip_geometry(document, scene_id),
        excluded_ids,
        layer,
        start_frame,
        duration_frames,
    ))
}

fn scene_clip_geometry(
    document: &ProjectDocument,
    scene_id: i32,
) -> Vec<AviQtlTimelineClipGeometry> {
    document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene_id)
        .map(|clip| AviQtlTimelineClipGeometry {
            clip_id: clip.id,
            layer: clip.layer,
            start_frame: clip.start,
            duration_frames: clip.duration,
        })
        .collect()
}

fn clip_document_geometry(clips: &[ClipDocument]) -> Vec<AviQtlTimelineClipGeometry> {
    clips
        .iter()
        .map(|clip| AviQtlTimelineClipGeometry {
            clip_id: clip.id,
            layer: clip.layer,
            start_frame: clip.start,
            duration_frames: clip.duration,
        })
        .collect()
}

fn clip_geometry_updates(geometry: Vec<AviQtlTimelineClipGeometry>) -> Vec<ClipGeometryUpdate> {
    geometry
        .into_iter()
        .map(|clip| ClipGeometryUpdate {
            clip_id: clip.clip_id,
            layer: clip.layer,
            start: clip.start_frame,
            duration: clip.duration_frames,
        })
        .collect()
}

/// Errors produced while loading or editing an authoritative timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineError {
    /// The project or edit request is not valid JSON for the supported schema.
    InvalidJson,
    /// The project file version is outside the supported range.
    UnsupportedVersion,
    /// The edit or ID allocation request is invalid for the current state.
    InvalidArgument,
    /// The transaction no longer matches the current state.
    Conflict,
}

impl Display for TimelineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidJson => "invalid project or edit JSON",
            Self::UnsupportedVersion => "unsupported project version",
            Self::InvalidArgument => "invalid timeline operation",
            Self::Conflict => "timeline state conflict",
        })
    }
}

impl Error for TimelineError {}

impl From<StateError> for TimelineError {
    fn from(error: StateError) -> Self {
        match error {
            StateError::InvalidJson => Self::InvalidJson,
            StateError::UnsupportedVersion => Self::UnsupportedVersion,
            StateError::InvalidArgument => Self::InvalidArgument,
            StateError::Conflict => Self::Conflict,
        }
    }
}

/// An opaque reversible edit produced by [`TimelineState::plan`] or
/// [`TimelineState::plan_batch`].
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineTransaction {
    inner: Transaction,
}

impl TimelineTransaction {
    /// Combines already-applied transactions into one undo entry.
    pub fn combine(
        transactions: Vec<TimelineTransaction>,
    ) -> Result<TimelineTransaction, TimelineError> {
        let inner = crate::timeline_state::combine_transactions(
            transactions
                .into_iter()
                .map(|transaction| transaction.inner)
                .collect(),
        )?;
        Ok(Self { inner })
    }
}

/// Rust-owned authoritative project and timeline state.
#[derive(Debug, Clone)]
pub struct TimelineState {
    inner: CoreTimelineState,
}

impl TimelineState {
    /// Parses and normalizes a project document, deriving future IDs from its contents.
    pub fn from_json(input: &[u8]) -> Result<Self, TimelineError> {
        Self::from_json_with_id_hints(input, 1, 1)
    }

    /// Parses and normalizes a project document with caller-provided allocation hints.
    /// Existing IDs are always skipped even when a hint is stale.
    pub fn from_json_with_id_hints(
        input: &[u8],
        next_clip_hint: i32,
        next_scene_hint: i32,
    ) -> Result<Self, TimelineError> {
        let document = parse_project_document(input).map_err(StateError::from)?;
        let inner = CoreTimelineState::new(document, next_clip_hint, next_scene_hint)?;
        Ok(Self { inner })
    }

    /// Returns a typed snapshot of the authoritative state.
    pub fn snapshot(&self) -> ProjectDocument {
        self.inner.snapshot_document()
    }

    /// Returns a normalized JSON snapshot for compatibility adapters and file serialization.
    pub fn snapshot_json(&self) -> Result<Value, TimelineError> {
        self.inner.snapshot_json().map_err(Into::into)
    }

    /// Plans one typed reversible edit without mutating the state.
    pub fn plan(&self, command: TimelineCommand) -> Result<TimelineTransaction, TimelineError> {
        let inner = self.inner.plan(command)?;
        Ok(TimelineTransaction { inner })
    }

    /// Plans multiple edits atomically. Later requests observe the effects of earlier requests.
    pub fn plan_batch(
        &self,
        commands: Vec<TimelineCommand>,
    ) -> Result<TimelineTransaction, TimelineError> {
        let inner = self.inner.plan_batch(commands)?;
        Ok(TimelineTransaction { inner })
    }

    /// Applies a planned edit.
    pub fn apply(&mut self, transaction: &TimelineTransaction) -> Result<(), TimelineError> {
        self.inner
            .apply_transaction(&transaction.inner, true)
            .map_err(Into::into)
    }

    /// Applies a transaction's inverse patch.
    pub fn undo(&mut self, transaction: &TimelineTransaction) -> Result<(), TimelineError> {
        self.inner
            .apply_transaction(&transaction.inner, false)
            .map_err(Into::into)
    }

    /// Reserves clip IDs that do not collide with the current document.
    pub fn reserve_clip_ids(&mut self, count: usize) -> Result<Vec<i32>, TimelineError> {
        self.inner
            .reserve_ids(EntityKind::Clip, count)
            .map_err(Into::into)
    }

    /// Reserves scene IDs that do not collide with the current document.
    pub fn reserve_scene_ids(&mut self, count: usize) -> Result<Vec<i32>, TimelineError> {
        self.inner
            .reserve_ids(EntityKind::Scene, count)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn safe_package_api_preserves_catalog_repository_and_install_policy() {
        let mut catalog = PackageCatalog::default();
        let repositories = vec![json!({
            "url": "https://example.invalid",
            "enabled": true,
            "priority": 10
        })];
        let repository = repositories[0].as_object().expect("repository object");
        let installed = json!({"effect.blur":{"version":"1.0.0"}});
        catalog.merge(
            &[json!({
                "id": "effect.blur",
                "type": "effect",
                "version": "2.0.0",
                "display_name": {"en": "Blur"},
                "metadata_url": "https://example.invalid/blur.json"
            })],
            repository,
            &repositories,
            installed.as_object().expect("installed object"),
            "en",
            "0.6.2",
        );

        assert_eq!(catalog.packages_by_type("effect").len(), 1);
        assert_eq!(catalog.packages_by_type("installed").len(), 1);
        assert!(catalog.has_updates());
        assert_eq!(catalog.upgrade_ids(), ["effect.blur"]);
        assert_eq!(
            catalog
                .find("effect.blur", "")
                .and_then(|package| package.get("display_name").cloned()),
            Some(json!("Blur"))
        );

        let (repositories, changed) = mutate_package_repositories(
            &repositories,
            "https://second.invalid",
            PackageRepositoryOperation::Add {
                enabled: true,
                priority: 5,
            },
        )
        .expect("repository operation");
        assert!(changed);
        assert_eq!(
            enabled_package_repositories(&repositories)[0]["url"],
            "https://second.invalid"
        );

        let detail = json!({
            "type": "effect",
            "versions": [{
                "version": "2.0.0",
                "download_url": "https://example.invalid/blur.zip",
                "download_sha256": "abcd",
                "min_app_version": "0.6.0"
            }]
        });
        let normalized = normalize_package_metadata(detail.as_object().expect("detail object"))
            .expect("metadata normalizes");
        let selection = select_package_install(&normalized, "", "0.6.2");
        assert_eq!(selection.status, "ok");
        assert_eq!(selection.version, "2.0.0");
        assert_eq!(selection.package_type, "effect");
        assert!(package_id_is_valid("effect.blur"));
        assert!(!package_id_is_valid("../effect.blur"));
        assert!(package_type_is_installable("mod"));
        assert!(!package_type_is_installable("application"));
        assert!(package_archive_path_is_safe("wrapper/../effect/main.qml"));
        assert!(!package_archive_path_is_safe("../../main.qml"));

        assert!(catalog.set_installed("effect.blur", Some("2.0.0")));
        assert!(!catalog.has_updates());
    }

    #[test]
    fn safe_clipboard_api_preserves_layout_and_moves_past_collisions() {
        let document = TimelineState::from_json(
            br#"{
                "version": 3,
                "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
                "scenes": [{"id": 1, "name": "Root", "duration": 300}],
                "clips": [
                    {"id": 1, "sceneId": 1, "type": "rect", "start": 10, "duration": 10, "layer": 3}
                ]
            }"#,
        )
        .expect("clipboard fixture loads")
        .snapshot();
        let mut first = document.clips[0].clone();
        first.id = 10;
        first.start = 0;
        first.layer = 0;
        let mut second = first.clone();
        second.id = 11;
        second.start = 15;
        second.layer = 2;
        second.duration = 5;
        let clipboard = [first, second];

        assert_eq!(clipboard_duration(&clipboard), 20);
        let (safe_frame, planned) =
            plan_clipboard_paste(&document, 1, &clipboard, 10, 3).expect("paste is planned");
        assert_eq!(safe_frame, 20);
        assert_eq!(planned[0].start, 20);
        assert_eq!(planned[0].layer, 3);
        assert_eq!(planned[1].start, 35);
        assert_eq!(planned[1].layer, 5);

        let inserted = plan_scene_layer_insertion(&document, 1, 3, 2, true)
            .expect("layer insertion is planned");
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0].layer, 5);

        let shifted =
            plan_scene_layer_shift(&document, 1, 3, 3, -2).expect("layer shift is planned");
        assert_eq!(shifted.len(), 1);
        assert_eq!(shifted[0].layer, 1);
    }
}
