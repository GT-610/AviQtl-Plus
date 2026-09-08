use crate::audio_plugin::{AudioPluginAddition, AudioPluginCatalog, AudioPluginHydration};
use crate::effect_catalog::EffectCatalog;
use crate::effect_selection::EffectSelection;
use crate::media_import::{MediaImportKind, plan_media_import};
use crate::missing_media::{MissingMediaEntry, find_missing_media, plan_media_relink};
use crate::object_settings::{ObjectSettings, project_object_settings, replace_value_payload};
use crate::preset_store::PresetStore;
use crate::project_io::{ProjectDefaults, ProjectSession};
use crate::selection::{ClipSelection, SelectionBox};
use crate::timeline_interaction::{TimelineDragRequest, plan_timeline_drag};
use crate::transport::Transport;
use aviqtl_rust_core::api::{
    ClipDocument, EffectDocument, EffectInsertion, EffectMetadata, EffectPreset,
    MAX_TIMELINE_LAYER, MAX_TIMELINE_LAYERS, ProjectDocument, ProjectSettings, SceneDocument,
    TimelineCommand, TimelineTransaction, clipboard_duration, evaluate_keyframe_track,
    find_vacant_scene_frame, inspect_keyframe_track, plan_clip_delta_move, plan_clipboard_paste,
    plan_effect_reorder, plan_scene_layer_insertion, plan_scene_layer_shift, snap_scene_frame,
    timeline_duration as core_timeline_duration,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_UNDO_LIMIT: usize = 32;
const MIN_TIMELINE_VIEW_FRAMES: i32 = 100;
const TIMELINE_TAIL_PADDING_FRAMES: i32 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTab {
    pub id: i32,
    pub name: String,
    pub selected: bool,
    pub root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineClip {
    pub id: i32,
    pub label: String,
    pub start: i32,
    pub duration: i32,
    pub layer: i32,
    pub audio: bool,
    pub clip_by_upper_object: bool,
    pub selected: bool,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSettingsInput {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub sample_rate: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneSettingsInput {
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: i32,
    pub grid_mode: String,
    pub grid_bpm: f64,
    pub grid_offset: f64,
    pub grid_interval: i32,
    pub grid_subdivision: i32,
    pub enable_snap: bool,
    pub magnetic_snap_range: i32,
}

/// Framework-neutral state and commands for one open project workspace.
pub struct WorkspaceModel {
    project: ProjectSession,
    selected_scene: i32,
    selection: ClipSelection,
    effect_selection: EffectSelection,
    playhead: i32,
    transport: Transport,
    undo: Vec<TimelineTransaction>,
    redo: Vec<TimelineTransaction>,
    clip_clipboard: Vec<ClipDocument>,
    undo_limit: usize,
    document_revision: u64,
    status: String,
}

impl WorkspaceModel {
    pub fn new(project: ProjectSession) -> Self {
        let selected_scene = project.document.scenes.first().map_or(1, |scene| scene.id);
        Self {
            project,
            selected_scene,
            selection: ClipSelection::default(),
            effect_selection: EffectSelection::default(),
            playhead: 0,
            transport: Transport::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            clip_clipboard: Vec::new(),
            undo_limit: DEFAULT_UNDO_LIMIT,
            document_revision: 0,
            status: "Ready".to_owned(),
        }
    }

    pub fn project(&self) -> &ProjectSession {
        &self.project
    }

    pub fn project_mut(&mut self) -> &mut ProjectSession {
        &mut self.project
    }

    pub fn document(&self) -> &ProjectDocument {
        &self.project.document
    }

    pub fn document_revision(&self) -> u64 {
        self.document_revision
    }

    pub fn selected_scene(&self) -> i32 {
        self.selected_scene
    }

    pub fn selected_scene_document(&self) -> Option<&SceneDocument> {
        self.project
            .document
            .scenes
            .iter()
            .find(|scene| scene.id == self.selected_scene)
    }

    pub fn scene_tabs(&self) -> Vec<SceneTab> {
        let root_scene = self.project.document.scenes.first().map(|scene| scene.id);
        self.project
            .document
            .scenes
            .iter()
            .map(|scene| SceneTab {
                id: scene.id,
                name: scene.name.clone(),
                selected: scene.id == self.selected_scene,
                root: Some(scene.id) == root_scene,
            })
            .collect()
    }

    pub fn timeline_clips(&self) -> Vec<TimelineClip> {
        self.project
            .document
            .clips
            .iter()
            .filter(|clip| clip.scene_id == self.selected_scene)
            .map(|clip| TimelineClip {
                id: clip.id,
                label: clip
                    .effects
                    .first()
                    .map(|effect| effect.name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| clip.clip_type.clone()),
                start: clip.start,
                duration: clip.duration,
                layer: clip.layer,
                audio: clip.clip_type == "audio",
                clip_by_upper_object: clip.clip_by_upper_object,
                selected: self.selection.is_visually_selected(clip.id),
                primary: self.selection.primary() == Some(clip.id),
            })
            .collect()
    }

    pub fn timeline_duration(&self) -> i32 {
        core_timeline_duration(
            self.project
                .document
                .clips
                .iter()
                .filter(|clip| clip.scene_id == self.selected_scene),
        )
    }

    pub fn timeline_view_duration(&self) -> i32 {
        self.scene_timing()
            .1
            .max(
                self.timeline_duration()
                    .saturating_add(TIMELINE_TAIL_PADDING_FRAMES),
            )
            .max(MIN_TIMELINE_VIEW_FRAMES)
    }

    pub fn missing_media(&self) -> Vec<MissingMediaEntry> {
        find_missing_media(&self.project.document, self.project.path.as_deref())
    }

    pub fn relink_media(&mut self, clip_id: i32, path: &Path) -> bool {
        let plan = match plan_media_relink(&self.project.document, clip_id, path) {
            Ok(plan) => plan,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        let display_path = plan.path.clone();
        if self.execute(TimelineCommand::SetEffectParameter {
            clip_id: plan.clip_id,
            effect_index: plan.effect_index,
            param_name: plan.parameter.to_owned(),
            value: Value::String(plan.path),
            media_duration_seconds: plan.media_duration_seconds,
        }) {
            self.status = format!("Relinked media to {display_path}");
            true
        } else {
            false
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn playhead(&self) -> i32 {
        self.playhead
    }

    pub fn is_playing(&self) -> bool {
        self.transport.is_playing()
    }

    pub fn playback_speed(&self) -> f64 {
        self.transport.playback_speed()
    }

    pub fn selected_clip_ids(&self) -> &[i32] {
        self.selection.ids()
    }

    pub fn primary_clip_id(&self) -> Option<i32> {
        self.selection.primary()
    }

    pub fn selected_layer(&self) -> i32 {
        self.selection.selected_layer()
    }

    pub fn selected_clip_document(&self) -> Option<&ClipDocument> {
        let clip_id = self.selection.primary()?;
        self.project
            .document
            .clips
            .iter()
            .find(|clip| clip.id == clip_id && clip.scene_id == self.selected_scene)
    }

    pub fn selected_effect_index(&self) -> Option<usize> {
        self.effect_selection.current().or_else(|| {
            self.selected_clip_document()
                .filter(|clip| self.object_settings_item_count(clip) > 0)
                .map(|_| 0)
        })
    }

    pub fn object_settings_uses_audio_plugins(&self) -> bool {
        self.selected_clip_document()
            .is_some_and(|clip| clip.clip_type == "audio")
    }

    pub fn object_settings(&self, catalog: &EffectCatalog) -> Option<ObjectSettings> {
        let clip = self.selected_clip_document()?;
        Some(project_object_settings(
            &self.project.document,
            clip,
            self.selected_scene,
            self.playhead,
            catalog,
            |index| self.effect_is_selected(index),
        ))
    }

    pub fn effect_is_selected(&self, index: usize) -> bool {
        self.effect_selection.is_selected(index)
            || (self.effect_selection.current().is_none() && index == 0)
    }

    pub fn select_effect(&mut self, index: usize, control: bool, shift: bool) -> bool {
        let Some(clip) = self.selected_clip_document() else {
            return false;
        };
        if index >= self.object_settings_item_count(clip) {
            return false;
        }
        self.effect_selection.click(index, control, shift);
        true
    }

    pub fn context_select_effect(&mut self, index: usize) -> bool {
        let Some(clip) = self.selected_clip_document() else {
            return false;
        };
        if index >= self.object_settings_item_count(clip) {
            return false;
        }
        self.effect_selection.context_click(index);
        true
    }

    pub fn add_audio_plugin(&mut self, addition: AudioPluginAddition) -> bool {
        let Some((clip_id, index)) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .map(|clip| (clip.id, clip.audio_plugins.len()))
        else {
            self.status = "Select an audio object before adding a plugin".to_owned();
            return false;
        };
        let display_name = addition.display_name;
        if self.execute(TimelineCommand::InsertAudioPlugin {
            clip_id,
            index,
            plugin: addition.plugin,
        }) {
            self.effect_selection.click(index, false, false);
            self.status = format!("Added audio plugin {display_name}");
            true
        } else {
            false
        }
    }

    pub fn hydrate_audio_plugins(&mut self, catalog: &AudioPluginCatalog) -> AudioPluginHydration {
        let pending = catalog.pending_hydration_count(&self.project.document);
        if pending > 0 && (!self.undo.is_empty() || !self.redo.is_empty()) {
            self.status = format!(
                "Deferred restoring {pending} audio plugin(s) until the project is reopened"
            );
            return AudioPluginHydration {
                hydrated: 0,
                deferred: pending,
                errors: Vec::new(),
            };
        }
        let mut document = self.project.document.clone();
        let result = catalog.hydrate_document(&mut document);
        if result.hydrated == 0 {
            return result;
        }
        let applied = self
            .project
            .state
            .plan(TimelineCommand::ReplaceDocument { document })
            .and_then(|transaction| self.project.state.apply(&transaction));
        match applied {
            Ok(()) => {
                self.project.refresh();
                self.reconcile_after_edit();
                self.document_revision = self.document_revision.wrapping_add(1);
                self.status = format!("Restored {} project audio plugin(s)", result.hydrated);
                result
            }
            Err(error) => AudioPluginHydration {
                hydrated: 0,
                deferred: 0,
                errors: vec![error.to_string()],
            },
        }
    }

    pub fn reorder_audio_plugins(&mut self, source: usize, target: usize) -> bool {
        let Some((clip_id, length)) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .map(|clip| (clip.id, clip.audio_plugins.len()))
        else {
            return false;
        };
        if source >= length || target >= length {
            return false;
        }
        let permutation = match plan_effect_reorder(length, &[source], target, 0) {
            Ok(permutation) => permutation,
            Err(error) => {
                self.status = error.to_string();
                return false;
            }
        };
        if permutation.iter().copied().eq(0..length) {
            return false;
        }
        if self.execute(TimelineCommand::ReorderAudioPlugins {
            clip_id,
            permutation: permutation.clone(),
        }) {
            self.effect_selection.apply_permutation(&permutation);
            self.status = "Reordered audio plugin".to_owned();
            true
        } else {
            false
        }
    }

    pub fn set_audio_plugin_enabled(&mut self, plugin_index: usize, enabled: bool) -> bool {
        let Some((clip_id, length)) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .map(|clip| (clip.id, clip.audio_plugins.len()))
        else {
            return false;
        };
        if plugin_index >= length {
            return false;
        }
        let targets = self.effect_selection.action_targets(plugin_index);
        let count = targets.len();
        for target in targets {
            if !self.execute(TimelineCommand::SetAudioPluginEnabled {
                clip_id,
                plugin_index: target,
                enabled,
            }) {
                return false;
            }
        }
        self.status = format!("Updated {count} audio plugin(s)");
        true
    }

    pub fn remove_audio_plugin(&mut self, plugin_index: usize) -> bool {
        self.remove_audio_plugin_indices(vec![plugin_index])
    }

    pub fn remove_audio_plugin_group(&mut self, plugin_index: usize) -> bool {
        let Some(length) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .map(|clip| clip.audio_plugins.len())
        else {
            return false;
        };
        if plugin_index >= length {
            return false;
        }
        self.remove_audio_plugin_indices(self.effect_selection.action_targets(plugin_index))
    }

    pub fn remove_selected_audio_plugins(&mut self) -> bool {
        let mut targets = self.effect_selection.deletion_targets();
        if targets.is_empty()
            && let Some(index) = self.selected_effect_index()
        {
            targets.push(index);
        }
        self.remove_audio_plugin_indices(targets)
    }

    fn remove_audio_plugin_indices(&mut self, mut plugin_indices: Vec<usize>) -> bool {
        let Some((clip_id, length)) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .map(|clip| (clip.id, clip.audio_plugins.len()))
        else {
            return false;
        };
        plugin_indices.retain(|index| *index < length);
        plugin_indices.sort_unstable();
        plugin_indices.dedup();
        if plugin_indices.is_empty() {
            return false;
        }
        let mut next_selection = self.effect_selection.clone();
        next_selection.apply_removals(&plugin_indices);
        let count = plugin_indices.len();
        let mut changed = false;
        for plugin_index in plugin_indices.into_iter().rev() {
            if !self.execute(TimelineCommand::RemoveAudioPlugin {
                clip_id,
                plugin_index,
            }) {
                return changed;
            }
            changed = true;
        }
        if changed {
            self.effect_selection = next_selection;
            self.status = format!("Removed {count} audio plugin(s)");
        }
        changed
    }

    pub fn add_effect(&mut self, catalog: &EffectCatalog, effect_id: &str) -> bool {
        let Some(effect) = catalog.effect_document(effect_id) else {
            self.status = format!("Effect {effect_id} is not available");
            return false;
        };
        let Some((clip_id, index)) = self
            .selected_clip_document()
            .map(|clip| (clip.id, clip.effects.len()))
        else {
            return false;
        };
        let name = effect.name.clone();
        if self.execute(TimelineCommand::InsertEffects {
            clip_id,
            insertions: vec![EffectInsertion { index, effect }],
        }) {
            self.effect_selection.click(index, false, false);
            self.status = format!("Added effect {name}");
            true
        } else {
            false
        }
    }

    pub fn reorder_effects(&mut self, source: usize, target: usize) -> bool {
        let Some((clip_id, length, transform_first)) = self.selected_clip_document().map(|clip| {
            (
                clip.id,
                clip.effects.len(),
                clip.effects
                    .first()
                    .is_some_and(|effect| effect.id == "transform"),
            )
        }) else {
            return false;
        };
        if source >= length || target >= length {
            return false;
        }
        let indices = self.effect_selection.action_targets(source);
        let permutation =
            match plan_effect_reorder(length, &indices, target, usize::from(transform_first)) {
                Ok(permutation) => permutation,
                Err(error) => {
                    self.status = error.to_string();
                    return false;
                }
            };
        if permutation.iter().copied().eq(0..length) {
            return false;
        }
        if self.execute(TimelineCommand::ReorderEffects {
            clip_id,
            permutation: permutation.clone(),
        }) {
            self.effect_selection.apply_permutation(&permutation);
            self.status = format!("Reordered {} effect(s)", indices.len());
            true
        } else {
            false
        }
    }

    pub fn save_effect_preset(
        &mut self,
        store: &PresetStore,
        effect_index: usize,
        name: &str,
    ) -> bool {
        let Some(effect) = self
            .selected_clip_document()
            .and_then(|clip| clip.effects.get(effect_index))
        else {
            return false;
        };
        let effect_id = effect.id.clone();
        let result = store.save(
            &effect_id,
            name,
            effect.params.clone(),
            effect.keyframes.clone().unwrap_or_default(),
            effect.enabled,
        );
        match result {
            Ok(()) => {
                self.status = format!("Saved preset {name}");
                true
            }
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    pub fn load_effect_preset(
        &mut self,
        store: &PresetStore,
        effect_index: usize,
        name: &str,
    ) -> bool {
        let Some(clip) = self.selected_clip_document().cloned() else {
            return false;
        };
        let Some(effect_id) = clip
            .effects
            .get(effect_index)
            .map(|effect| effect.id.clone())
        else {
            return false;
        };
        let preset = match store.load_preset(&effect_id, name) {
            Ok(preset) => preset,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        let commands = match effect_preset_commands(&clip, effect_index, &preset) {
            Ok(commands) => commands,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        for command in commands {
            if !self.execute(command) {
                return false;
            }
        }
        self.status = format!("Loaded preset {}", preset.name);
        true
    }

    pub fn delete_effect_preset(
        &mut self,
        store: &PresetStore,
        effect_index: usize,
        name: &str,
    ) -> bool {
        let Some(effect_id) = self
            .selected_clip_document()
            .and_then(|clip| clip.effects.get(effect_index))
            .map(|effect| effect.id.clone())
        else {
            return false;
        };
        match store.delete(&effect_id, name) {
            Ok(()) => {
                self.status = format!("Deleted preset {name}");
                true
            }
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    pub fn save_audio_plugin_preset(
        &mut self,
        store: &PresetStore,
        plugin_index: usize,
        name: &str,
    ) -> bool {
        let Some(plugin) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .and_then(|clip| clip.audio_plugins.get(plugin_index))
        else {
            return false;
        };
        let plugin_id = plugin.id.clone();
        let result = store.save(
            &plugin_id,
            name,
            plugin.params.clone(),
            plugin.keyframes.clone().unwrap_or_default(),
            plugin.enabled,
        );
        match result {
            Ok(()) => {
                self.status = format!("Saved preset {name}");
                true
            }
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    pub fn load_audio_plugin_preset(
        &mut self,
        store: &PresetStore,
        plugin_index: usize,
        name: &str,
    ) -> bool {
        let Some(clip) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .cloned()
        else {
            return false;
        };
        let Some(plugin_id) = clip
            .audio_plugins
            .get(plugin_index)
            .map(|plugin| plugin.id.clone())
        else {
            return false;
        };
        let preset = match store.load_preset(&plugin_id, name) {
            Ok(preset) => preset,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        let commands = match audio_plugin_preset_commands(&clip, plugin_index, &preset) {
            Ok(commands) => commands,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        for command in commands {
            if !self.execute(command) {
                return false;
            }
        }
        self.status = format!("Loaded preset {}", preset.name);
        true
    }

    pub fn delete_audio_plugin_preset(
        &mut self,
        store: &PresetStore,
        plugin_index: usize,
        name: &str,
    ) -> bool {
        let Some(plugin_id) = self
            .selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .and_then(|clip| clip.audio_plugins.get(plugin_index))
            .map(|plugin| plugin.id.clone())
        else {
            return false;
        };
        match store.delete(&plugin_id, name) {
            Ok(()) => {
                self.status = format!("Deleted preset {name}");
                true
            }
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    pub fn set_effect_enabled(&mut self, effect_index: usize, enabled: bool) -> bool {
        let Some((clip_id, length)) = self
            .selected_clip_document()
            .map(|clip| (clip.id, clip.effects.len()))
        else {
            return false;
        };
        if effect_index >= length {
            return false;
        }
        let targets = self.effect_selection.action_targets(effect_index);
        let count = targets.len();
        for target in targets {
            if !self.execute(TimelineCommand::SetEffectEnabled {
                clip_id,
                effect_index: target,
                enabled,
            }) {
                return false;
            }
        }
        self.status = format!("Updated {count} effect(s)");
        true
    }

    pub fn remove_effect(&mut self, effect_index: usize) -> bool {
        self.remove_effect_indices(vec![effect_index])
    }

    pub fn remove_effect_group(&mut self, effect_index: usize) -> bool {
        let Some(length) = self.selected_clip_document().map(|clip| clip.effects.len()) else {
            return false;
        };
        if effect_index >= length {
            return false;
        }
        self.remove_effect_indices(self.effect_selection.action_targets(effect_index))
    }

    pub fn remove_selected_effects(&mut self) -> bool {
        let mut targets = self.effect_selection.deletion_targets();
        if targets.is_empty()
            && let Some(index) = self.selected_effect_index()
        {
            targets.push(index);
        }
        self.remove_effect_indices(targets)
    }

    fn remove_effect_indices(&mut self, mut effect_indices: Vec<usize>) -> bool {
        let Some((clip_id, length, transform_first)) = self.selected_clip_document().map(|clip| {
            (
                clip.id,
                clip.effects.len(),
                clip.effects
                    .first()
                    .is_some_and(|effect| effect.id == "transform"),
            )
        }) else {
            return false;
        };
        effect_indices.retain(|index| *index < length && !(transform_first && *index == 0));
        effect_indices.sort_unstable();
        effect_indices.dedup();
        if effect_indices.is_empty() {
            return false;
        }
        let mut next_selection = self.effect_selection.clone();
        next_selection.apply_removals(&effect_indices);
        let count = effect_indices.len();
        if self.execute(TimelineCommand::RemoveEffects {
            clip_id,
            effect_indices,
        }) {
            self.effect_selection = next_selection;
            self.status = format!("Removed {count} effect(s)");
            true
        } else {
            false
        }
    }

    pub fn set_effect_parameter(
        &mut self,
        effect_index: usize,
        param_name: &str,
        value: Value,
    ) -> bool {
        let Some(relative_frame) = self.selected_clip_document().map(|clip| {
            self.playhead
                .saturating_sub(clip.start)
                .clamp(0, clip.duration.max(0))
        }) else {
            return false;
        };
        self.set_effect_parameter_at_frame(effect_index, param_name, relative_frame, value)
    }

    pub fn set_effect_parameter_at_frame(
        &mut self,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        value: Value,
    ) -> bool {
        let Some((clip, effect, original)) =
            self.effect_parameter_context(effect_index, param_name)
        else {
            return false;
        };
        let value = replace_value_payload(&original, value);
        let frame = frame.clamp(0, clip.duration.max(0));
        let track = effect
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        let command = if track.is_some() {
            let points = inspect_keyframe_track(track, &original, clip.duration);
            let options = keyframe_options_at(&points, frame, "linear");
            TimelineCommand::SetEffectKeyframe {
                clip_id: clip.id,
                effect_index,
                param_name: param_name.to_owned(),
                frame,
                value,
                options,
            }
        } else {
            TimelineCommand::SetEffectParameter {
                clip_id: clip.id,
                effect_index,
                param_name: param_name.to_owned(),
                value,
                media_duration_seconds: None,
            }
        };
        self.execute(command)
    }

    pub fn add_effect_keyframe(
        &mut self,
        effect_index: usize,
        param_name: &str,
        frame: i32,
    ) -> bool {
        self.add_effect_keyframe_with_options(effect_index, param_name, frame, true)
    }

    fn add_effect_keyframe_with_options(
        &mut self,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        inherit_options: bool,
    ) -> bool {
        let Some((clip, effect, fallback)) =
            self.effect_parameter_context(effect_index, param_name)
        else {
            return false;
        };
        let frame = frame.clamp(0, clip.duration.max(0));
        let track = effect
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        let points = inspect_keyframe_track(track, &fallback, clip.duration);
        if points.iter().any(|point| point.frame == frame) {
            return false;
        }
        let value = evaluate_keyframe_track(track, &fallback, clip.duration, frame);
        let options = if inherit_options {
            keyframe_options_at(&points, frame, "none")
        } else {
            json!({"interp": "none"})
        };
        self.execute(TimelineCommand::SetEffectKeyframe {
            clip_id: clip.id,
            effect_index,
            param_name: param_name.to_owned(),
            frame,
            value,
            options,
        })
    }

    pub fn remove_effect_keyframe(
        &mut self,
        effect_index: usize,
        param_name: &str,
        frame: i32,
    ) -> bool {
        let Some((clip, effect, fallback)) =
            self.effect_parameter_context(effect_index, param_name)
        else {
            return false;
        };
        if frame <= 0 || frame > clip.duration.max(0) {
            return false;
        }
        let track = effect
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        if !inspect_keyframe_track(track, &fallback, clip.duration)
            .iter()
            .any(|point| point.frame == frame)
        {
            return false;
        }
        self.execute(TimelineCommand::RemoveEffectKeyframe {
            clip_id: clip.id,
            effect_index,
            param_name: param_name.to_owned(),
            frame,
        })
    }

    pub fn move_effect_keyframe(
        &mut self,
        effect_index: usize,
        param_name: &str,
        old_frame: i32,
        new_frame: i32,
    ) -> bool {
        let Some((clip, effect, fallback)) =
            self.effect_parameter_context(effect_index, param_name)
        else {
            return false;
        };
        let duration = clip.duration.max(0);
        let new_frame = new_frame.clamp(0, duration);
        if old_frame <= 0 || old_frame > duration || old_frame == new_frame {
            return false;
        }
        let points = inspect_keyframe_track(
            effect
                .keyframes
                .as_ref()
                .and_then(|tracks| tracks.get(param_name)),
            &fallback,
            duration,
        );
        if !points.iter().any(|point| point.frame == old_frame)
            || points.iter().any(|point| point.frame == new_frame)
        {
            return false;
        }
        self.execute(TimelineCommand::MoveEffectKeyframe {
            clip_id: clip.id,
            effect_index,
            param_name: param_name.to_owned(),
            old_frame,
            new_frame,
        })
    }

    pub fn set_audio_plugin_parameter_at_frame(
        &mut self,
        plugin_index: usize,
        param_name: &str,
        frame: i32,
        value: Value,
    ) -> bool {
        let Some((clip, plugin, original)) =
            self.audio_plugin_parameter_context(plugin_index, param_name)
        else {
            return false;
        };
        let value = replace_value_payload(&original, value);
        let frame = frame.clamp(0, clip.duration.max(0));
        let track = plugin
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        let command = if track.is_some() {
            TimelineCommand::SetAudioPluginKeyframe {
                clip_id: clip.id,
                plugin_index,
                param_name: param_name.to_owned(),
                frame,
                value,
                options: json!({"interp": "linear"}),
            }
        } else {
            TimelineCommand::SetAudioPluginParameter {
                clip_id: clip.id,
                plugin_index,
                param_name: param_name.to_owned(),
                value,
            }
        };
        self.execute(command)
    }

    pub fn seed_audio_plugin_keyframes(&mut self, plugin_index: usize, param_name: &str) -> bool {
        let Some((clip, plugin, fallback)) =
            self.audio_plugin_parameter_context(plugin_index, param_name)
        else {
            return false;
        };
        let duration = clip.duration.max(0);
        let track = plugin
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        if !inspect_keyframe_track(track, &fallback, duration).is_empty() {
            return false;
        }
        let start_value = evaluate_keyframe_track(track, &fallback, duration, 0);
        if !self.execute(TimelineCommand::SetAudioPluginKeyframe {
            clip_id: clip.id,
            plugin_index,
            param_name: param_name.to_owned(),
            frame: 0,
            value: start_value,
            options: json!({"interp": "linear"}),
        }) {
            return false;
        }
        if duration > 0 {
            let end_value = evaluate_keyframe_track(track, &fallback, duration, duration);
            if !self.execute(TimelineCommand::SetAudioPluginKeyframe {
                clip_id: clip.id,
                plugin_index,
                param_name: param_name.to_owned(),
                frame: duration,
                value: end_value,
                options: json!({"interp": "linear"}),
            }) {
                return false;
            }
        }
        self.status = format!("Enabled keyframes for audio plugin parameter {param_name}");
        true
    }

    pub fn add_audio_plugin_keyframe(
        &mut self,
        plugin_index: usize,
        param_name: &str,
        frame: i32,
    ) -> bool {
        let Some((clip, plugin, fallback)) =
            self.audio_plugin_parameter_context(plugin_index, param_name)
        else {
            return false;
        };
        let duration = clip.duration.max(0);
        let frame = frame.clamp(0, duration);
        let track = plugin
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        if inspect_keyframe_track(track, &fallback, duration)
            .iter()
            .any(|point| point.frame == frame)
        {
            return false;
        }
        let value = evaluate_keyframe_track(track, &fallback, duration, frame);
        self.execute(TimelineCommand::SetAudioPluginKeyframe {
            clip_id: clip.id,
            plugin_index,
            param_name: param_name.to_owned(),
            frame,
            value,
            options: json!({"interp": "linear"}),
        })
    }

    pub fn remove_audio_plugin_keyframe(
        &mut self,
        plugin_index: usize,
        param_name: &str,
        frame: i32,
    ) -> bool {
        let Some((clip, plugin, fallback)) =
            self.audio_plugin_parameter_context(plugin_index, param_name)
        else {
            return false;
        };
        let duration = clip.duration.max(0);
        if frame <= 0 || frame >= duration {
            return false;
        }
        let track = plugin
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        if !inspect_keyframe_track(track, &fallback, duration)
            .iter()
            .any(|point| point.frame == frame)
        {
            return false;
        }
        self.execute(TimelineCommand::RemoveAudioPluginKeyframe {
            clip_id: clip.id,
            plugin_index,
            param_name: param_name.to_owned(),
            frame,
        })
    }

    pub fn move_audio_plugin_keyframe(
        &mut self,
        plugin_index: usize,
        param_name: &str,
        old_frame: i32,
        new_frame: i32,
    ) -> bool {
        let Some((clip, plugin, fallback)) =
            self.audio_plugin_parameter_context(plugin_index, param_name)
        else {
            return false;
        };
        let duration = clip.duration.max(0);
        let new_frame = new_frame.clamp(0, duration);
        if old_frame <= 0 || old_frame >= duration || old_frame == new_frame {
            return false;
        }
        let points = inspect_keyframe_track(
            plugin
                .keyframes
                .as_ref()
                .and_then(|tracks| tracks.get(param_name)),
            &fallback,
            duration,
        );
        if !points.iter().any(|point| point.frame == old_frame)
            || points.iter().any(|point| point.frame == new_frame)
        {
            return false;
        }
        self.execute(TimelineCommand::MoveAudioPluginKeyframe {
            clip_id: clip.id,
            plugin_index,
            param_name: param_name.to_owned(),
            old_frame,
            new_frame,
        })
    }

    pub fn prepare_effect_easing(
        &mut self,
        effect_index: usize,
        param_name: &str,
        start_frame: i32,
        end_frame: i32,
    ) -> Option<aviqtl_rust_core::api::KeyframePoint> {
        let duration = self.selected_clip_document()?.duration.max(0);
        let mut required = vec![start_frame.clamp(0, duration)];
        if end_frame != duration {
            required.push(end_frame.clamp(0, duration));
        }
        required.push(duration);
        required.sort_unstable();
        required.dedup();
        for frame in required {
            let exists = self
                .effect_parameter_context(effect_index, param_name)
                .is_some_and(|(clip, effect, fallback)| {
                    inspect_keyframe_track(
                        effect
                            .keyframes
                            .as_ref()
                            .and_then(|tracks| tracks.get(param_name)),
                        &fallback,
                        clip.duration,
                    )
                    .iter()
                    .any(|point| point.frame == frame)
                });
            if !exists {
                let inherit_options = frame != duration;
                if !self.add_effect_keyframe_with_options(
                    effect_index,
                    param_name,
                    frame,
                    inherit_options,
                ) {
                    return None;
                }
            }
        }
        let (clip, effect, fallback) = self.effect_parameter_context(effect_index, param_name)?;
        inspect_keyframe_track(
            effect
                .keyframes
                .as_ref()
                .and_then(|tracks| tracks.get(param_name)),
            &fallback,
            clip.duration,
        )
        .into_iter()
        .find(|point| point.frame == start_frame.clamp(0, duration))
    }

    pub fn set_effect_keyframe_options(
        &mut self,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        options: Value,
    ) -> bool {
        let Some((clip, effect, fallback)) =
            self.effect_parameter_context(effect_index, param_name)
        else {
            return false;
        };
        let frame = frame.clamp(0, clip.duration.max(0));
        let Some(point) = inspect_keyframe_track(
            effect
                .keyframes
                .as_ref()
                .and_then(|tracks| tracks.get(param_name)),
            &fallback,
            clip.duration,
        )
        .into_iter()
        .find(|point| point.frame == frame) else {
            return false;
        };
        self.execute(TimelineCommand::SetEffectKeyframe {
            clip_id: clip.id,
            effect_index,
            param_name: param_name.to_owned(),
            frame,
            value: point.value,
            options,
        })
    }

    pub fn snap_effect_keyframe_frame(
        &self,
        relative_frame: f64,
        timeline_scale: f64,
        enable_snap: bool,
    ) -> i32 {
        let Some(clip) = self.selected_clip_document() else {
            return relative_frame.round().clamp(0.0, f64::from(i32::MAX)) as i32;
        };
        let absolute_frame = f64::from(clip.start) + relative_frame;
        let snapped = if enable_snap {
            self.selected_scene_document().map_or_else(
                || absolute_frame.round().clamp(0.0, f64::from(i32::MAX)) as i32,
                |scene| {
                    let mut scene = scene.clone();
                    scene.enable_snap = true;
                    snap_scene_frame(absolute_frame, false, &scene, timeline_scale)
                },
            )
        } else {
            absolute_frame.round().clamp(0.0, f64::from(i32::MAX)) as i32
        };
        snapped
            .saturating_sub(clip.start)
            .clamp(0, clip.duration.max(0))
    }

    pub fn seek_effect_frame(&mut self, relative_frame: i32) {
        if let Some((start, duration)) = self
            .selected_clip_document()
            .map(|clip| (clip.start, clip.duration.max(0)))
        {
            self.seek(start.saturating_add(relative_frame.clamp(0, duration)));
        }
    }

    fn effect_parameter_context(
        &self,
        effect_index: usize,
        param_name: &str,
    ) -> Option<(ClipDocument, aviqtl_rust_core::api::EffectDocument, Value)> {
        self.selected_clip_document().and_then(|clip| {
            clip.effects.get(effect_index).and_then(|effect| {
                effect
                    .params
                    .get(param_name)
                    .map(|original| (clip.clone(), effect.clone(), original.clone()))
            })
        })
    }

    fn audio_plugin_parameter_context(
        &self,
        plugin_index: usize,
        param_name: &str,
    ) -> Option<(
        ClipDocument,
        aviqtl_rust_core::api::AudioPluginDocument,
        Value,
    )> {
        self.selected_clip_document()
            .filter(|clip| clip.clip_type == "audio")
            .and_then(|clip| {
                clip.audio_plugins.get(plugin_index).and_then(|plugin| {
                    plugin
                        .params
                        .get(param_name)
                        .map(|original| (clip.clone(), plugin.clone(), original.clone()))
                })
            })
    }

    fn object_settings_item_count(&self, clip: &ClipDocument) -> usize {
        if clip.clip_type == "audio" {
            clip.audio_plugins.len()
        } else {
            clip.effects.len()
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn set_undo_limit(&mut self, limit: usize) {
        self.undo_limit = limit.max(1);
        self.trim_undo_history();
    }

    pub fn project_settings(&self) -> ProjectSettingsInput {
        let settings = &self.project.document.settings;
        ProjectSettingsInput {
            width: settings.width,
            height: settings.height,
            fps: settings.fps,
            sample_rate: settings.sample_rate,
        }
    }

    pub fn update_project_settings(&mut self, input: ProjectSettingsInput) -> bool {
        let current = &self.project.document.settings;
        let settings = ProjectSettings {
            width: input.width.clamp(1, 8_000),
            height: input.height.clamp(1, 8_000),
            fps: bounded_f64(input.fps, 1.0, 240.0, 60.0),
            sample_rate: input.sample_rate.clamp(8_000, 192_000),
            extra: current.extra.clone(),
        };
        match self
            .project
            .state
            .plan(TimelineCommand::UpdateProjectSettings { settings })
            .and_then(|transaction| self.project.state.apply(&transaction))
        {
            Ok(()) => {
                self.project.refresh();
                self.reconcile_after_edit();
                self.project.dirty = true;
                self.document_revision = self.document_revision.wrapping_add(1);
                self.status = "Project settings applied".to_owned();
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn selected_scene_settings(&self) -> Option<SceneSettingsInput> {
        self.selected_scene_document().map(scene_settings_input)
    }

    pub fn scene_settings(&self, scene_id: i32) -> Option<SceneSettingsInput> {
        self.project
            .document
            .scenes
            .iter()
            .find(|scene| scene.id == scene_id)
            .map(scene_settings_input)
    }

    pub fn create_scene(
        &mut self,
        defaults: ProjectDefaults,
        input: SceneSettingsInput,
    ) -> Option<i32> {
        let scene_id = match self.project.state.reserve_scene_ids(1) {
            Ok(ids) => ids[0],
            Err(error) => {
                self.status = error.to_string();
                return None;
            }
        };
        let name = input.name.clone();
        let scene = SceneDocument {
            id: scene_id,
            name: name.clone(),
            width: defaults.width,
            height: defaults.height,
            fps: defaults.fps,
            start: 0,
            duration: 300,
            nested_duration: 0,
            locked_layers: Vec::new(),
            hidden_layers: Vec::new(),
            grid_mode: "Auto".to_owned(),
            grid_bpm: 120.0,
            grid_offset: 0.0,
            grid_interval: 10,
            grid_subdivision: 4,
            enable_snap: true,
            magnetic_snap_range: 10,
            extra: BTreeMap::new(),
        };
        if !self.execute(TimelineCommand::InsertScene { index: None, scene }) {
            return None;
        }
        self.selected_scene = scene_id;
        self.selection.clear();
        self.effect_selection.clear();
        self.playhead = 0;
        self.transport.pause();
        if self.update_scene_settings(scene_id, input) {
            self.status = format!("Created scene {name}");
        }
        Some(scene_id)
    }

    pub fn update_scene_settings(&mut self, scene_id: i32, input: SceneSettingsInput) -> bool {
        let Some(mut scene) = self
            .project
            .document
            .scenes
            .iter()
            .find(|scene| scene.id == scene_id)
            .cloned()
        else {
            self.status = format!("Scene #{scene_id} does not exist");
            return false;
        };
        let input = normalized_scene_settings(input);
        scene.name = input.name;
        scene.width = input.width;
        scene.height = input.height;
        scene.fps = input.fps;
        scene.duration = input.duration;
        scene.grid_mode = input.grid_mode;
        scene.grid_bpm = input.grid_bpm;
        scene.grid_offset = input.grid_offset;
        scene.grid_interval = input.grid_interval;
        scene.grid_subdivision = input.grid_subdivision;
        scene.enable_snap = input.enable_snap;
        scene.magnetic_snap_range = input.magnetic_snap_range;
        let name = scene.name.clone();
        if self.execute(TimelineCommand::UpdateScene { scene_id, scene }) {
            self.selected_scene = scene_id;
            self.status = format!("Updated scene {name}");
            true
        } else {
            false
        }
    }

    pub fn execute(&mut self, command: TimelineCommand) -> bool {
        match self.project.state.plan(command).and_then(|transaction| {
            self.project.state.apply(&transaction)?;
            Ok(transaction)
        }) {
            Ok(transaction) => {
                self.finish_timeline_edit(transaction, "Edit applied");
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn execute_batch(&mut self, commands: Vec<TimelineCommand>) -> bool {
        if commands.is_empty() {
            return false;
        }
        match self
            .project
            .state
            .plan_batch(commands)
            .and_then(|transaction| {
                self.project.state.apply(&transaction)?;
                Ok(transaction)
            }) {
            Ok(transaction) => {
                self.finish_timeline_edit(transaction, "Edit group applied");
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(transaction) = self.undo.pop() else {
            return false;
        };
        match self.project.state.undo(&transaction) {
            Ok(()) => {
                self.redo.push(transaction);
                self.refresh_after_history_change("Undo");
                true
            }
            Err(error) => {
                self.undo.push(transaction);
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn redo(&mut self) -> bool {
        let Some(transaction) = self.redo.pop() else {
            return false;
        };
        match self.project.state.apply(&transaction) {
            Ok(()) => {
                self.undo.push(transaction);
                self.trim_undo_history();
                self.refresh_after_history_change("Redo");
                true
            }
            Err(error) => {
                self.redo.push(transaction);
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn switch_scene(&mut self, scene_id: i32) -> bool {
        if !self
            .project
            .document
            .scenes
            .iter()
            .any(|scene| scene.id == scene_id)
        {
            return false;
        }
        self.selected_scene = scene_id;
        self.selection.clear();
        self.effect_selection.clear();
        self.playhead = 0;
        self.transport.pause();
        true
    }

    pub fn remove_scene(&mut self, scene_id: i32) -> bool {
        let root_scene = self.project.document.scenes.first().map(|scene| scene.id);
        if self.project.document.scenes.len() <= 1 || root_scene == Some(scene_id) {
            self.status = "The root scene cannot be removed".to_owned();
            return false;
        }
        if !self
            .project
            .document
            .scenes
            .iter()
            .any(|scene| scene.id == scene_id)
        {
            return false;
        }
        let removed_selected_scene = self.selected_scene == scene_id;
        if !self.execute(TimelineCommand::RemoveScene { scene_id }) {
            return false;
        }
        if removed_selected_scene {
            self.selected_scene = self.project.document.scenes[0].id;
            self.selection.clear();
            self.effect_selection.clear();
            self.playhead = 0;
            self.transport.pause();
        }
        true
    }

    pub fn select_layer(&mut self, layer: i32) {
        self.selection.set_selected_layer(layer);
        self.selection.clear();
    }

    pub fn click_clip(&mut self, clip_id: i32, control: bool) {
        self.selection.click_clip(clip_id, control);
        self.reconcile_effect_selection();
    }

    pub fn prepare_clip_drag(&mut self, clip_id: i32, control: bool) {
        if !self.selection.is_selected(clip_id) {
            self.selection.click_clip(clip_id, control);
            self.reconcile_effect_selection();
        }
    }

    pub fn context_click_clip(&mut self, clip_id: i32) {
        if !self.selection.is_selected(clip_id) {
            self.selection.click_clip(clip_id, false);
        }
        self.reconcile_effect_selection();
    }

    pub fn toggle_clip_by_upper_object(&mut self, clip_id: i32) -> bool {
        let Some(clip) = self
            .project
            .document
            .clips
            .iter()
            .find(|clip| clip.id == clip_id && clip.scene_id == self.selected_scene)
        else {
            return false;
        };
        self.execute(TimelineCommand::SetClipByUpperObject {
            clip_id,
            enabled: !clip.clip_by_upper_object,
        })
    }

    pub fn preview_box_selection(&mut self, selection_box: SelectionBox) {
        self.selection
            .preview_box(&self.project.document, self.selected_scene, selection_box);
    }

    pub fn finish_box_selection(&mut self) {
        self.selection.finish_preview();
        self.reconcile_effect_selection();
    }

    pub fn cancel_box_selection(&mut self) {
        self.selection.cancel_preview();
    }

    pub fn remove_selected_clips(&mut self) -> bool {
        let commands = self
            .selection
            .ids()
            .iter()
            .copied()
            .map(|clip_id| TimelineCommand::RemoveClip { clip_id })
            .collect::<Vec<_>>();
        if self.execute_batch(commands) {
            self.selection.clear();
            self.effect_selection.clear();
            true
        } else {
            false
        }
    }

    pub fn split_selected_clips_at(&mut self, frame: i32) -> bool {
        let clips = self
            .selection
            .ids()
            .iter()
            .filter_map(|clip_id| {
                self.project.document.clips.iter().find(|clip| {
                    clip.id == *clip_id
                        && clip.scene_id == self.selected_scene
                        && frame > clip.start
                        && frame < clip.start.saturating_add(clip.duration)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if clips.is_empty() {
            self.status =
                "Move the edit frame inside at least one selected clip before splitting".to_owned();
            return false;
        }
        let new_clip_ids = match self.project.state.reserve_clip_ids(clips.len()) {
            Ok(ids) => ids,
            Err(error) => {
                self.status = error.to_string();
                return false;
            }
        };
        self.execute_batch(
            clips
                .into_iter()
                .zip(new_clip_ids)
                .map(|(clip, new_clip_id)| TimelineCommand::SplitClip {
                    clip_id: clip.id,
                    frame,
                    new_clip_id,
                })
                .collect(),
        )
    }

    pub fn copy_selected_clips(&mut self) -> bool {
        let copied = self
            .selection
            .ids()
            .iter()
            .filter_map(|clip_id| {
                self.project
                    .document
                    .clips
                    .iter()
                    .find(|clip| clip.id == *clip_id && clip.scene_id == self.selected_scene)
            })
            .cloned()
            .collect::<Vec<_>>();
        if copied.is_empty() {
            return false;
        }
        self.clip_clipboard = copied;
        self.status = format!("Copied {} clip(s)", self.clip_clipboard.len());
        true
    }

    pub fn cut_selected_clips(&mut self) -> bool {
        if !self.copy_selected_clips() {
            return false;
        }
        let commands = self
            .selection
            .ids()
            .iter()
            .copied()
            .map(|clip_id| TimelineCommand::RemoveClip { clip_id })
            .collect();
        if self.execute_batch(commands) {
            self.selection.clear();
            self.effect_selection.clear();
            self.status = format!("Cut {} clip(s)", self.clip_clipboard.len());
            true
        } else {
            false
        }
    }

    pub fn paste_clips_at(
        &mut self,
        requested_frame: i32,
        requested_layer: i32,
        maximum_layers: i32,
    ) -> Option<(i32, i32)> {
        let maximum_layers = maximum_layers.clamp(1, MAX_TIMELINE_LAYERS);
        if self.clip_clipboard.is_empty() {
            return None;
        }
        let duration = clipboard_duration(&self.clip_clipboard);
        let (safe_frame, geometry) = match plan_clipboard_paste(
            &self.project.document,
            self.selected_scene,
            &self.clip_clipboard,
            requested_frame,
            requested_layer,
        ) {
            Ok(planned) => planned,
            Err(error) => {
                self.status = error.to_string();
                return None;
            }
        };
        if geometry.iter().any(|entry| entry.layer >= maximum_layers) {
            self.status = format!("Pasted clips exceed the {maximum_layers}-layer timeline");
            return None;
        }
        let ids = match self
            .project
            .state
            .reserve_clip_ids(self.clip_clipboard.len())
        {
            Ok(ids) => ids,
            Err(error) => {
                self.status = error.to_string();
                return None;
            }
        };
        let commands = self
            .clip_clipboard
            .iter()
            .cloned()
            .zip(geometry)
            .zip(ids)
            .map(|((mut clip, geometry), clip_id)| {
                clip.id = clip_id;
                clip.scene_id = self.selected_scene;
                clip.layer = geometry.layer;
                clip.start = geometry.start;
                clip.duration = geometry.duration;
                TimelineCommand::InsertClip { index: None, clip }
            })
            .collect();
        if !self.execute_batch(commands) {
            return None;
        }
        self.status = format!("Pasted {} clip(s)", self.clip_clipboard.len());
        Some((
            safe_frame.saturating_add(duration),
            requested_layer.clamp(0, maximum_layers - 1),
        ))
    }

    pub fn duplicate_selected_clips_at(
        &mut self,
        frame: i32,
        layer: i32,
        maximum_layers: i32,
    ) -> Option<(i32, i32)> {
        self.copy_selected_clips();
        self.paste_clips_at(frame, layer, maximum_layers)
    }

    pub fn insert_catalog_object_at(
        &mut self,
        object_id: &str,
        requested_start: i32,
        target_layer: i32,
        default_duration_frames: i32,
        catalog: &EffectCatalog,
    ) -> bool {
        let Some(object) = catalog
            .find(object_id)
            .filter(|entry| entry.kind == "object")
        else {
            self.status = format!("Object {object_id} is not available");
            return false;
        };
        let transform = (object.id != "audio")
            .then(|| catalog.find("transform"))
            .flatten();
        if object.id != "audio" && transform.is_none() {
            self.status = "Standard drawing effect is not available".to_owned();
            return false;
        }
        let layer = target_layer.clamp(0, MAX_TIMELINE_LAYER);
        if self
            .selected_scene_document()
            .is_some_and(|scene| scene.locked_layers.contains(&layer))
        {
            self.status = format!("Layer {} is locked", layer + 1);
            return false;
        }
        let duration = default_duration_frames.max(1);
        let start = match find_vacant_scene_frame(
            &self.project.document,
            self.selected_scene,
            &[],
            layer,
            requested_start.max(0),
            duration,
        ) {
            Ok(start) => start,
            Err(error) => {
                self.status = error.to_string();
                return false;
            }
        };
        let clip_id = match self.project.state.reserve_clip_ids(1) {
            Ok(ids) => ids[0],
            Err(error) => {
                self.status = error.to_string();
                return false;
            }
        };
        let target_scene_id = (object.id == "scene").then(|| {
            self.project
                .document
                .scenes
                .iter()
                .find(|scene| scene.id != self.selected_scene)
                .map_or(-1, |scene| scene.id)
        });
        let clip = object_clip_from_metadata(
            clip_id,
            self.selected_scene,
            start,
            layer,
            duration,
            object,
            transform,
            target_scene_id,
        );
        if !self.execute(TimelineCommand::InsertClip { index: None, clip }) {
            return false;
        }
        self.selection.replace([clip_id]);
        self.reconcile_effect_selection();
        self.status = format!("Added object {}", object.name);
        true
    }

    pub fn import_media_files(
        &mut self,
        paths: &[PathBuf],
        mut frame: i32,
        mut layer: i32,
        default_duration_frames: i32,
        maximum_layers: i32,
        catalog: &EffectCatalog,
    ) -> Option<(i32, i32)> {
        let mut imported = 0usize;
        let mut errors = Vec::new();
        for path in paths {
            match self.import_media_file(
                path,
                frame,
                layer,
                default_duration_frames,
                maximum_layers,
                catalog,
            ) {
                Ok((next_frame, target_layer)) => {
                    frame = next_frame;
                    layer = target_layer;
                    imported += 1;
                }
                Err(error) => errors.push(error),
            }
        }
        self.status = match (imported, errors.is_empty()) {
            (0, true) => "No media files were dropped".to_owned(),
            (0, false) => errors.join("; "),
            (count, true) => format!("Imported {count} media file(s)"),
            (count, false) => format!("Imported {count} media file(s); {}", errors.join("; ")),
        };
        (imported > 0).then_some((frame, layer))
    }

    fn import_media_file(
        &mut self,
        path: &Path,
        requested_start: i32,
        target_layer: i32,
        default_duration_frames: i32,
        maximum_layers: i32,
        catalog: &EffectCatalog,
    ) -> Result<(i32, i32), String> {
        let scene_fps = self
            .selected_scene_document()
            .map_or(60.0, |scene| scene.fps);
        let plan = plan_media_import(path, scene_fps, default_duration_frames.max(1))?;
        let transform = catalog.find("transform");
        let path = plan.path.to_string_lossy().into_owned();
        let duration = plan.duration_frames;
        let scene_id = self.selected_scene;
        let maximum_layers = maximum_layers.clamp(1, MAX_TIMELINE_LAYERS);
        let layer = target_layer.clamp(0, maximum_layers - 1);
        let start = match plan.kind {
            MediaImportKind::Video => find_vacant_linked_media_frame(
                &self.project.document,
                scene_id,
                layer,
                requested_start,
                duration,
                maximum_layers,
            )?,
            MediaImportKind::Audio | MediaImportKind::Image => find_vacant_scene_frame(
                &self.project.document,
                scene_id,
                &[],
                layer,
                requested_start.max(0),
                duration,
            )
            .map_err(|error| error.to_string())?,
        };

        let commands = match plan.kind {
            MediaImportKind::Video => {
                let transform = transform
                    .ok_or_else(|| "Standard drawing effect is not available".to_owned())?;
                let video = catalog
                    .find("video")
                    .ok_or_else(|| "Video object is not available".to_owned())?;
                let audio = catalog
                    .find("audio")
                    .ok_or_else(|| "Audio object is not available".to_owned())?;
                let ids = self
                    .project
                    .state
                    .reserve_clip_ids(2)
                    .map_err(|error| error.to_string())?;
                vec![
                    TimelineCommand::InsertClip {
                        index: None,
                        clip: media_clip_from_metadata(
                            ids[0],
                            scene_id,
                            start,
                            layer,
                            duration,
                            video,
                            Some(transform),
                            "path",
                            &path,
                            false,
                        ),
                    },
                    TimelineCommand::InsertClip {
                        index: None,
                        clip: media_clip_from_metadata(
                            ids[1],
                            scene_id,
                            start,
                            layer.saturating_add(1),
                            duration,
                            audio,
                            None,
                            "source",
                            &path,
                            true,
                        ),
                    },
                ]
            }
            MediaImportKind::Audio | MediaImportKind::Image => {
                let object_id = if plan.kind == MediaImportKind::Audio {
                    "audio"
                } else {
                    "image"
                };
                let path_parameter = if plan.kind == MediaImportKind::Audio {
                    "source"
                } else {
                    "path"
                };
                let object = catalog
                    .find(object_id)
                    .ok_or_else(|| format!("{object_id} object is not available"))?;
                let transform = if plan.kind == MediaImportKind::Image {
                    Some(
                        transform
                            .ok_or_else(|| "Standard drawing effect is not available".to_owned())?,
                    )
                } else {
                    None
                };
                let ids = self
                    .project
                    .state
                    .reserve_clip_ids(1)
                    .map_err(|error| error.to_string())?;
                vec![TimelineCommand::InsertClip {
                    index: None,
                    clip: media_clip_from_metadata(
                        ids[0],
                        scene_id,
                        start,
                        layer,
                        duration,
                        object,
                        transform,
                        path_parameter,
                        &path,
                        false,
                    ),
                }]
            }
        };
        if !self.execute_batch(commands) {
            return Err(self.status.clone());
        }
        Ok((start.saturating_add(duration), layer))
    }

    pub fn move_selected_clips(
        &mut self,
        delta_layer: i32,
        delta_frame: i32,
        maximum_layers: i32,
    ) -> bool {
        let ids = self.selection.ids().to_vec();
        if ids.is_empty() {
            return false;
        }
        let maximum_layer = maximum_layers.clamp(1, MAX_TIMELINE_LAYERS) - 1;
        let selected_layers = self
            .project
            .document
            .clips
            .iter()
            .filter(|clip| ids.contains(&clip.id))
            .map(|clip| clip.layer)
            .collect::<Vec<_>>();
        let minimum_selected_layer = selected_layers.iter().copied().min().unwrap_or(0);
        let maximum_selected_layer = selected_layers.iter().copied().max().unwrap_or(0);
        let delta_layer = delta_layer.clamp(
            -minimum_selected_layer,
            maximum_layer.saturating_sub(maximum_selected_layer),
        );
        match plan_clip_delta_move(
            &self.project.document,
            self.selected_scene,
            &ids,
            delta_layer,
            delta_frame,
        ) {
            Ok(updates) => self.execute_geometry_updates(updates),
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn drag_selected_clips(&mut self, request: TimelineDragRequest) -> bool {
        let plan = match plan_timeline_drag(
            &self.project.document,
            self.selected_scene,
            self.selection.ids(),
            request,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        self.execute_geometry_updates(plan.updates)
    }

    pub fn toggle_layer_visibility(&mut self, layer: i32) -> bool {
        let Some(scene) = self.selected_scene_document() else {
            return false;
        };
        self.set_layer_visibility(layer, scene.hidden_layers.contains(&layer))
    }

    pub fn set_layer_visibility(&mut self, layer: i32, visible: bool) -> bool {
        let Some(mut scene) = self.selected_scene_document().cloned() else {
            return false;
        };
        scene.hidden_layers.retain(|hidden| *hidden != layer);
        if !visible {
            scene.hidden_layers.push(layer);
            scene.hidden_layers.sort_unstable();
            scene.hidden_layers.dedup();
        }
        self.execute(TimelineCommand::UpdateScene {
            scene_id: scene.id,
            scene,
        })
    }

    pub fn set_all_layers_visible(&mut self, visible: bool, maximum_layers: i32) -> bool {
        let Some(mut scene) = self.selected_scene_document().cloned() else {
            return false;
        };
        scene.hidden_layers = if visible {
            Vec::new()
        } else {
            (0..maximum_layers.clamp(1, MAX_TIMELINE_LAYERS)).collect()
        };
        self.execute(TimelineCommand::UpdateScene {
            scene_id: scene.id,
            scene,
        })
    }

    pub fn toggle_layer_lock(&mut self, layer: i32) -> bool {
        let Some(mut scene) = self.selected_scene_document().cloned() else {
            return false;
        };
        if scene.locked_layers.contains(&layer) {
            scene.locked_layers.retain(|locked| *locked != layer);
        } else {
            scene.locked_layers.push(layer);
            scene.locked_layers.sort_unstable();
            scene.locked_layers.dedup();
        }
        self.execute(TimelineCommand::UpdateScene {
            scene_id: scene.id,
            scene,
        })
    }

    pub fn insert_layers(
        &mut self,
        target_layer: i32,
        count: i32,
        above: bool,
        maximum_layers: i32,
    ) -> bool {
        match plan_scene_layer_insertion(
            &self.project.document,
            self.selected_scene,
            target_layer,
            count,
            above,
        ) {
            Ok(updates)
                if updates
                    .iter()
                    .all(|update| update.layer < maximum_layers.clamp(1, MAX_TIMELINE_LAYERS)) =>
            {
                self.execute_geometry_updates(updates)
            }
            Ok(_) => {
                self.status = "Layer insertion exceeds the configured timeline".to_owned();
                false
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn shift_layers(
        &mut self,
        start_layer: i32,
        end_layer: i32,
        delta: i32,
        maximum_layers: i32,
    ) -> bool {
        match plan_scene_layer_shift(
            &self.project.document,
            self.selected_scene,
            start_layer,
            end_layer,
            delta,
        ) {
            Ok(updates)
                if updates
                    .iter()
                    .all(|update| update.layer < maximum_layers.clamp(1, MAX_TIMELINE_LAYERS)) =>
            {
                self.execute_geometry_updates(updates)
            }
            Ok(_) => {
                self.status = "Layer shift exceeds the configured timeline".to_owned();
                false
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn seek(&mut self, frame: i32) {
        self.playhead = frame.clamp(0, self.timeline_duration());
        self.transport.seek(Instant::now(), self.playhead);
    }

    pub fn set_edit_target(&mut self, frame: i32, layer: i32) {
        self.seek(frame);
        self.selection.set_selected_layer(layer);
    }

    pub fn snap_timeline_frame(&self, frame: f64, ignore_snap: bool, timeline_scale: f64) -> i32 {
        self.selected_scene_document().map_or_else(
            || frame.round().clamp(0.0, f64::from(i32::MAX)) as i32,
            |scene| snap_scene_frame(frame, ignore_snap, scene, timeline_scale),
        )
    }

    pub fn set_playback_speed(&mut self, speed: f64) {
        self.transport.set_playback_speed(speed);
    }

    pub fn pause_playback(&mut self) {
        self.transport.pause();
    }

    pub fn toggle_playback(&mut self) {
        let end_frame = self.timeline_duration();
        self.transport
            .toggle(Instant::now(), &mut self.playhead, end_frame);
    }

    pub fn step_playhead(&mut self, delta: i32) {
        let end_frame = self.timeline_duration();
        self.transport.step(delta, &mut self.playhead, end_frame);
    }

    pub fn begin_scrub(&mut self) {
        self.transport.begin_scrub();
    }

    pub fn scrub_to(&mut self, frame: i32) -> bool {
        let end_frame = self.timeline_duration();
        self.transport.scrub_to(
            Instant::now(),
            frame.clamp(0, end_frame),
            &mut self.playhead,
        )
    }

    pub fn end_scrub(&mut self) {
        self.transport.end_scrub(Instant::now(), self.playhead);
    }

    pub fn update_transport(&mut self, now: Instant) -> bool {
        let fps = self.scene_timing().0;
        let end_frame = self.timeline_duration();
        self.transport
            .update(now, fps, end_frame, &mut self.playhead)
    }

    fn execute_geometry_updates(
        &mut self,
        updates: Vec<aviqtl_rust_core::api::ClipGeometryUpdate>,
    ) -> bool {
        match updates.as_slice() {
            [] => false,
            [update] => self.execute(TimelineCommand::UpdateClipGeometry {
                clip_id: update.clip_id,
                layer: update.layer,
                start: update.start,
                duration: update.duration,
            }),
            _ => self.execute(TimelineCommand::BatchUpdateClipGeometry { updates }),
        }
    }

    fn finish_timeline_edit(&mut self, transaction: TimelineTransaction, status: &str) {
        self.undo.push(transaction);
        self.trim_undo_history();
        self.redo.clear();
        self.project.refresh();
        self.reconcile_after_edit();
        self.project.dirty = true;
        self.document_revision = self.document_revision.wrapping_add(1);
        self.status = status.to_owned();
    }

    fn refresh_after_history_change(&mut self, status: &str) {
        self.project.refresh();
        self.reconcile_after_edit();
        self.project.dirty = true;
        self.document_revision = self.document_revision.wrapping_add(1);
        self.status = status.to_owned();
    }

    fn reconcile_after_edit(&mut self) {
        if !self
            .project
            .document
            .scenes
            .iter()
            .any(|scene| scene.id == self.selected_scene)
        {
            self.selected_scene = self
                .project
                .document
                .scenes
                .first()
                .map_or(1, |scene| scene.id);
        }
        self.selection
            .reconcile(&self.project.document, self.selected_scene);
        self.reconcile_effect_selection();
        let end_frame = self.timeline_duration();
        self.playhead = self.playhead.clamp(0, end_frame);
    }

    fn reconcile_effect_selection(&mut self) {
        let Some(clip) = self.selection.primary().and_then(|clip_id| {
            self.project
                .document
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
        }) else {
            self.effect_selection.clear();
            return;
        };
        self.effect_selection
            .reconcile(clip.id, self.object_settings_item_count(clip));
    }

    fn scene_timing(&self) -> (f64, i32) {
        self.selected_scene_document().map_or(
            (
                self.project.document.settings.fps,
                self.project
                    .document
                    .scenes
                    .first()
                    .map_or(1, |scene| scene.duration),
            ),
            |scene| (scene.fps, scene.duration),
        )
    }

    fn trim_undo_history(&mut self) {
        let remove_count = self.undo.len().saturating_sub(self.undo_limit);
        if remove_count > 0 {
            self.undo.drain(..remove_count);
        }
    }
}

fn effect_preset_commands(
    clip: &ClipDocument,
    effect_index: usize,
    preset: &EffectPreset,
) -> Result<Vec<TimelineCommand>, String> {
    let Some(effect) = clip.effects.get(effect_index) else {
        return Err("The preset target changed before it could be applied".to_owned());
    };
    if effect.id != preset.effect_id {
        return Err("The preset target changed before it could be applied".to_owned());
    }
    let mut commands = preset
        .params
        .iter()
        .map(|(name, value)| TimelineCommand::SetEffectParameter {
            clip_id: clip.id,
            effect_index,
            param_name: name.clone(),
            value: value.clone(),
            media_duration_seconds: None,
        })
        .collect::<Vec<_>>();
    for (name, track) in &preset.keyframes {
        let fallback = preset
            .params
            .get(name)
            .or_else(|| effect.params.get(name))
            .cloned()
            .unwrap_or(Value::Null);
        commands.extend(
            inspect_keyframe_track(Some(track), &fallback, clip.duration)
                .into_iter()
                .map(|point| TimelineCommand::SetEffectKeyframe {
                    clip_id: clip.id,
                    effect_index,
                    param_name: name.clone(),
                    frame: point.frame,
                    value: point.value,
                    options: point.options,
                }),
        );
    }
    commands.push(TimelineCommand::SetEffectEnabled {
        clip_id: clip.id,
        effect_index,
        enabled: preset.enabled,
    });
    Ok(commands)
}

fn audio_plugin_preset_commands(
    clip: &ClipDocument,
    plugin_index: usize,
    preset: &EffectPreset,
) -> Result<Vec<TimelineCommand>, String> {
    let Some(plugin) = clip.audio_plugins.get(plugin_index) else {
        return Err("The preset target changed before it could be applied".to_owned());
    };
    if plugin.id != preset.effect_id {
        return Err("The preset target changed before it could be applied".to_owned());
    }
    let mut commands = preset
        .params
        .iter()
        .map(|(name, value)| TimelineCommand::SetAudioPluginParameter {
            clip_id: clip.id,
            plugin_index,
            param_name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    for (name, track) in &preset.keyframes {
        let fallback = preset
            .params
            .get(name)
            .or_else(|| plugin.params.get(name))
            .cloned()
            .unwrap_or(Value::Null);
        commands.extend(
            inspect_keyframe_track(Some(track), &fallback, clip.duration)
                .into_iter()
                .map(|point| TimelineCommand::SetAudioPluginKeyframe {
                    clip_id: clip.id,
                    plugin_index,
                    param_name: name.clone(),
                    frame: point.frame,
                    value: point.value,
                    options: point.options,
                }),
        );
    }
    commands.push(TimelineCommand::SetAudioPluginEnabled {
        clip_id: clip.id,
        plugin_index,
        enabled: preset.enabled,
    });
    Ok(commands)
}

fn effect_from_metadata(metadata: &EffectMetadata) -> EffectDocument {
    EffectDocument {
        id: metadata.id.clone(),
        name: metadata.name.clone(),
        enabled: true,
        params: metadata.params.clone(),
        keyframes: None,
        extra: BTreeMap::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn object_clip_from_metadata(
    clip_id: i32,
    scene_id: i32,
    start: i32,
    layer: i32,
    duration: i32,
    object: &EffectMetadata,
    transform: Option<&EffectMetadata>,
    target_scene_id: Option<i32>,
) -> ClipDocument {
    let mut effects = transform
        .into_iter()
        .map(effect_from_metadata)
        .collect::<Vec<_>>();
    let mut object_effect = effect_from_metadata(object);
    if let Some(target_scene_id) = target_scene_id {
        object_effect
            .params
            .insert("targetSceneId".to_owned(), Value::from(target_scene_id));
    }
    effects.push(object_effect);
    ClipDocument {
        id: clip_id,
        scene_id,
        clip_type: object.id.clone(),
        start,
        duration,
        layer,
        clip_by_upper_object: false,
        params: Map::new(),
        audio_plugins: Vec::new(),
        effects,
        extra: BTreeMap::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn media_clip_from_metadata(
    clip_id: i32,
    scene_id: i32,
    start: i32,
    layer: i32,
    duration: i32,
    object: &EffectMetadata,
    transform: Option<&EffectMetadata>,
    path_parameter: &str,
    path: &str,
    linked_video: bool,
) -> ClipDocument {
    let mut clip = object_clip_from_metadata(
        clip_id, scene_id, start, layer, duration, object, transform, None,
    );
    if let Some(object_effect) = clip
        .effects
        .iter_mut()
        .find(|effect| effect.id == object.id)
    {
        object_effect
            .params
            .insert(path_parameter.to_owned(), Value::from(path));
        if linked_video {
            object_effect
                .params
                .insert("linkedVideo".to_owned(), Value::Bool(true));
        }
    }
    clip
}

fn find_vacant_linked_media_frame(
    document: &ProjectDocument,
    scene_id: i32,
    video_layer: i32,
    requested_start: i32,
    duration: i32,
    maximum_layers: i32,
) -> Result<i32, String> {
    let audio_layer = video_layer
        .checked_add(1)
        .filter(|layer| *layer < maximum_layers.clamp(1, MAX_TIMELINE_LAYERS))
        .ok_or_else(|| "A linked video requires an audio layer below it".to_owned())?;
    let mut candidate = requested_start.max(0);
    for _ in 0..100 {
        let video_start =
            find_vacant_scene_frame(document, scene_id, &[], video_layer, candidate, duration)
                .map_err(|error| error.to_string())?;
        let audio_start =
            find_vacant_scene_frame(document, scene_id, &[], audio_layer, video_start, duration)
                .map_err(|error| error.to_string())?;
        if audio_start == video_start {
            return Ok(video_start);
        }
        candidate = audio_start;
    }
    let video_start =
        find_vacant_scene_frame(document, scene_id, &[], video_layer, candidate, duration)
            .map_err(|error| error.to_string())?;
    let audio_start =
        find_vacant_scene_frame(document, scene_id, &[], audio_layer, video_start, duration)
            .map_err(|error| error.to_string())?;
    Ok(if audio_start == video_start {
        video_start
    } else {
        candidate
    })
}

fn keyframe_options_at(
    points: &[aviqtl_rust_core::api::KeyframePoint],
    frame: i32,
    default_interpolation: &str,
) -> Value {
    points
        .iter()
        .rev()
        .find(|point| point.frame <= frame)
        .or_else(|| points.first())
        .map_or_else(
            || json!({"interp": default_interpolation}),
            |point| point.options.clone(),
        )
}

fn scene_settings_input(scene: &SceneDocument) -> SceneSettingsInput {
    SceneSettingsInput {
        name: scene.name.clone(),
        width: scene.width,
        height: scene.height,
        fps: scene.fps,
        duration: scene.duration,
        grid_mode: scene.grid_mode.clone(),
        grid_bpm: scene.grid_bpm,
        grid_offset: scene.grid_offset,
        grid_interval: scene.grid_interval,
        grid_subdivision: scene.grid_subdivision,
        enable_snap: scene.enable_snap,
        magnetic_snap_range: scene.magnetic_snap_range,
    }
}

fn normalized_scene_settings(mut input: SceneSettingsInput) -> SceneSettingsInput {
    input.width = bounded_positive_i32(input.width, 32_768, 1_920);
    input.height = bounded_positive_i32(input.height, 32_768, 1_080);
    input.fps = bounded_positive_f64(input.fps, 1_000.0, 60.0);
    input.duration = bounded_positive_i32(input.duration, i32::MAX, 300);
    input.grid_mode = match input.grid_mode.as_str() {
        "BPM" => "BPM",
        "Frame" => "Frame",
        _ => "Auto",
    }
    .to_owned();
    input.grid_bpm = bounded_positive_f64(input.grid_bpm, 1_000.0, 120.0);
    input.grid_offset =
        if input.grid_offset.is_finite() && (0.0..=86_400.0).contains(&input.grid_offset) {
            input.grid_offset
        } else {
            0.0
        };
    input.grid_interval = bounded_positive_i32(input.grid_interval, 1_000_000, 10);
    input.grid_subdivision = bounded_positive_i32(input.grid_subdivision, 128, 4);
    input.magnetic_snap_range = bounded_positive_i32(input.magnetic_snap_range, 100, 10);
    input
}

fn bounded_f64(value: f64, minimum: f64, maximum: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback
    }
}

fn bounded_positive_i32(value: i32, maximum: i32, fallback: i32) -> i32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::TimelineState;

    fn temporary_media_file(extension: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aviqtl-workspace-media-{}-{}.{}",
            std::process::id(),
            crate::recovery::generate_recovery_id(),
            extension
        ));
        std::fs::write(&path, []).expect("temporary media placeholder is writable");
        path
    }

    fn workspace() -> WorkspaceModel {
        let state = TimelineState::from_json(
            br#"{
                "version": 3,
                "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
                "scenes": [
                    {"id": 1, "name": "Root", "duration": 300},
                    {"id": 2, "name": "Scene 2", "duration": 180}
                ],
                "clips": [
                    {"id": 1, "sceneId": 1, "type": "rect", "start": 0, "duration": 20, "layer": 0},
                    {"id": 2, "sceneId": 1, "type": "text", "start": 30, "duration": 20, "layer": 1},
                    {"id": 3, "sceneId": 2, "type": "rect", "start": 0, "duration": 20, "layer": 0}
                ]
            }"#,
        )
        .expect("workspace fixture loads");
        let document = state.snapshot();
        WorkspaceModel::new(ProjectSession {
            state,
            document,
            path: None,
            dirty: false,
        })
    }

    fn workspace_with_effects() -> WorkspaceModel {
        let state = TimelineState::from_json(
            br#"{
                "version": 3,
                "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
                "scenes": [{"id": 1, "name": "Root", "duration": 300}],
                "clips": [{
                    "id": 1,
                    "sceneId": 1,
                    "type": "rect",
                    "start": 0,
                    "duration": 100,
                    "layer": 0,
                    "effects": [
                        {
                            "id": "rect",
                            "name": "Rectangle",
                            "params": {"count": {"$aviqtlType": "int", "value": 0, "extension": 7}}
                        },
                        {
                            "id": "blur",
                            "name": "Blur",
                            "params": {"size": 0},
                            "keyframes": {"size": [{"frame": 0, "value": 0}, {"frame": 20, "value": 20}]}
                        },
                        {
                            "id": "mosaic",
                            "name": "Mosaic",
                            "params": {"size": 10}
                        }
                    ]
                }]
            }"#,
        )
        .expect("effect workspace fixture loads");
        let document = state.snapshot();
        WorkspaceModel::new(ProjectSession {
            state,
            document,
            path: None,
            dirty: false,
        })
    }

    fn workspace_with_audio_plugins() -> WorkspaceModel {
        let state = TimelineState::from_json(
            br#"{
                "version":3,
                "settings":{"width":1920,"height":1080,"fps":60,"sampleRate":48000},
                "scenes":[{"id":1,"name":"Root","duration":300,"gridMode":"Frame","gridInterval":10}],
                "clips":[{
                    "id":9,"sceneId":1,"type":"audio","start":20,"duration":100,"layer":0,
                    "effects":[{"id":"audio","name":"Audio","params":{"volume":1.0}}],
                    "audioPlugins":[
                        {"id":"gain","enabled":true,"params":{"0":0.25}},
                        {"id":"delay","enabled":true,"params":{"0":0.5}}
                    ]
                }]
            }"#,
        )
        .expect("audio workspace fixture loads");
        let document = state.snapshot();
        WorkspaceModel::new(ProjectSession {
            state,
            document,
            path: None,
            dirty: false,
        })
    }

    #[test]
    fn scene_and_clip_snapshots_follow_the_active_workspace() {
        let mut workspace = workspace();
        assert_eq!(workspace.scene_tabs().len(), 2);
        assert_eq!(workspace.timeline_clips().len(), 2);
        assert_eq!(workspace.timeline_duration(), 50);
        assert!(!workspace.timeline_clips()[0].audio);
        assert!(!workspace.timeline_clips()[0].clip_by_upper_object);
        assert!(workspace.toggle_clip_by_upper_object(1));
        assert!(workspace.timeline_clips()[0].clip_by_upper_object);
        assert!(workspace.switch_scene(2));
        assert_eq!(workspace.timeline_clips().len(), 1);
        assert_eq!(workspace.timeline_clips()[0].id, 3);
        assert_eq!(workspace.timeline_duration(), 20);
        assert_eq!(workspace.playhead(), 0);

        let audio_workspace = workspace_with_audio_plugins();
        assert!(audio_workspace.timeline_clips()[0].audio);
    }

    #[test]
    fn empty_scene_timeline_duration_matches_qt() {
        let mut workspace = workspace();
        workspace.project_mut().document.clips.clear();
        assert_eq!(workspace.timeline_duration(), 1);
        assert_eq!(workspace.timeline_view_duration(), 300);

        workspace.seek(250);
        assert_eq!(workspace.playhead(), 1);

        workspace.begin_scrub();
        assert!(workspace.scrub_to(250));
        workspace.end_scrub();
        assert_eq!(workspace.playhead(), 1);

        workspace.seek(0);
        workspace.step_playhead(1);
        assert_eq!(workspace.playhead(), 1);
        workspace.step_playhead(1);
        assert_eq!(workspace.playhead(), 1);
    }

    #[test]
    fn timeline_view_duration_takes_scene_clip_tail_and_minimum() {
        let mut workspace = workspace();
        // Fixture scene 1: duration 300, clip end 50 -> max(300, 50+120, 100).
        assert_eq!(workspace.timeline_duration(), 50);
        assert_eq!(workspace.timeline_view_duration(), 300);

        // Transport clamps at the clip end, not the scene length.
        workspace.seek(250);
        assert_eq!(workspace.playhead(), 50);
        workspace.seek(0);
        workspace.step_playhead(1000);
        assert_eq!(workspace.playhead(), 50);
        workspace.begin_scrub();
        assert!(workspace.scrub_to(1000));
        workspace.end_scrub();
        assert_eq!(workspace.playhead(), 50);
        workspace.toggle_playback();
        assert!(workspace.is_playing());
        workspace.pause_playback();

        // Tail padding wins when clips extend past the scene length.
        workspace.project_mut().document.clips[1].start = 250;
        assert_eq!(workspace.timeline_duration(), 270);
        assert_eq!(workspace.timeline_view_duration(), 390);
    }

    #[test]
    fn playback_speed_clamps_to_the_qt_spinbox_range() {
        let mut workspace = workspace();
        workspace.set_playback_speed(0.05);
        assert_eq!(workspace.playback_speed(), 0.1);
        workspace.set_playback_speed(10.0);
        assert_eq!(workspace.playback_speed(), 4.0);
        workspace.set_playback_speed(2.5);
        assert_eq!(workspace.playback_speed(), 2.5);
    }

    #[test]
    fn selection_clipboard_and_history_match_the_qt_command_order() {
        let mut workspace = workspace();
        assert_eq!(workspace.document_revision(), 0);
        workspace.click_clip(1, false);
        workspace.click_clip(2, true);
        assert_eq!(workspace.selected_clip_ids(), [2, 1]);
        assert!(workspace.copy_selected_clips());
        assert_eq!(workspace.paste_clips_at(80, 4, 128), Some((130, 4)));
        assert_eq!(workspace.document().clips.len(), 5);
        assert_eq!(workspace.document_revision(), 1);
        assert!(workspace.can_undo());
        assert!(workspace.undo());
        assert_eq!(workspace.document().clips.len(), 3);
        assert_eq!(workspace.document_revision(), 2);
        assert!(workspace.redo());
        assert_eq!(workspace.document().clips.len(), 5);
        assert_eq!(workspace.document_revision(), 3);
    }

    #[test]
    fn clipboard_rejects_content_beyond_the_configured_layer_count() {
        let mut workspace = workspace();
        workspace.click_clip(1, false);
        workspace.click_clip(2, true);
        assert!(workspace.copy_selected_clips());
        let clip_count = workspace.document().clips.len();
        assert_eq!(workspace.paste_clips_at(80, 0, 1), None);
        assert_eq!(workspace.document().clips.len(), clip_count);
        assert!(workspace.status().contains("1-layer timeline"));
    }

    #[test]
    fn dropped_media_uses_qt_object_shapes_sequential_targets_and_undo_groups() {
        let (catalog, _) = EffectCatalog::load();
        let paths = [
            temporary_media_file("png"),
            temporary_media_file("wav"),
            temporary_media_file("mp4"),
        ];
        let mut workspace = workspace();

        assert_eq!(
            workspace.import_media_files(&paths, 60, 4, 15, 128, &catalog),
            Some((105, 4))
        );
        assert_eq!(workspace.status(), "Imported 3 media file(s)");
        assert_eq!(workspace.document_revision(), 3);
        assert_eq!(workspace.document().clips.len(), 7);

        let image = workspace
            .document()
            .clips
            .iter()
            .find(|clip| clip.clip_type == "image")
            .expect("image clip was imported");
        assert_eq!((image.start, image.duration, image.layer), (60, 15, 4));
        assert_eq!(image.effects[0].id, "transform");
        assert_eq!(image.effects[1].id, "image");
        assert_eq!(
            image.effects[1].params["path"],
            paths[0].to_string_lossy().as_ref()
        );

        let standalone_audio = workspace
            .document()
            .clips
            .iter()
            .find(|clip| clip.clip_type == "audio" && clip.start == 75)
            .expect("standalone audio clip was imported");
        assert_eq!((standalone_audio.duration, standalone_audio.layer), (15, 4));
        assert_eq!(
            standalone_audio.effects[0].params["source"],
            paths[1].to_string_lossy().as_ref()
        );

        let video = workspace
            .document()
            .clips
            .iter()
            .find(|clip| clip.clip_type == "video")
            .expect("video clip was imported");
        assert_eq!((video.start, video.duration, video.layer), (90, 15, 4));
        assert_eq!(video.effects[0].id, "transform");
        assert_eq!(video.effects[1].id, "video");
        let linked_audio = workspace
            .document()
            .clips
            .iter()
            .find(|clip| {
                clip.clip_type == "audio"
                    && clip.start == 90
                    && clip.effects[0].params["linkedVideo"] == true
            })
            .expect("linked audio clip was imported");
        assert_eq!((linked_audio.duration, linked_audio.layer), (15, 5));

        assert!(workspace.undo());
        assert_eq!(workspace.document().clips.len(), 5);
        assert!(workspace.undo());
        assert_eq!(workspace.document().clips.len(), 4);
        assert!(workspace.undo());
        assert_eq!(workspace.document().clips.len(), 3);

        for path in paths {
            std::fs::remove_file(path).expect("temporary media placeholder is removable");
        }
    }

    #[test]
    fn missing_media_relink_is_type_safe_dirty_and_undoable() {
        let missing_path = std::env::temp_dir().join(format!(
            "aviqtl-workspace-missing-{}-{}.png",
            std::process::id(),
            crate::recovery::generate_recovery_id()
        ));
        let replacement_path = temporary_media_file("png");
        let wrong_type_path = temporary_media_file("wav");
        let state = TimelineState::from_json(
            format!(
                r#"{{
                    "version": 3,
                    "settings": {{"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000}},
                    "scenes": [{{"id": 1, "name": "Root", "duration": 300}}],
                    "clips": [{{
                        "id": 7,
                        "sceneId": 1,
                        "type": "image",
                        "start": 0,
                        "duration": 100,
                        "layer": 2,
                        "effects": [{{"id": "image", "params": {{"path": {missing_path:?}}}}}]
                    }}]
                }}"#,
            )
            .as_bytes(),
        )
        .expect("missing-media workspace fixture loads");
        let document = state.snapshot();
        let mut workspace = WorkspaceModel::new(ProjectSession {
            state,
            document,
            path: None,
            dirty: false,
        });

        assert_eq!(workspace.missing_media().len(), 1);
        assert!(!workspace.relink_media(7, &wrong_type_path));
        assert_eq!(workspace.missing_media().len(), 1);
        assert!(workspace.relink_media(7, &replacement_path));
        assert!(workspace.project().dirty);
        assert!(workspace.missing_media().is_empty());
        assert_eq!(
            workspace.document().clips[0].effects[0].params["path"],
            replacement_path.to_string_lossy().as_ref()
        );
        assert!(workspace.undo());
        assert_eq!(workspace.missing_media().len(), 1);
        assert!(workspace.redo());
        assert!(workspace.missing_media().is_empty());

        std::fs::remove_file(replacement_path).expect("replacement fixture is removable");
        std::fs::remove_file(wrong_type_path).expect("wrong-type fixture is removable");
    }

    #[test]
    fn catalog_object_creation_uses_qt_defaults_selection_and_scene_target() {
        let (catalog, _) = EffectCatalog::load();
        let mut workspace = workspace();

        assert!(workspace.insert_catalog_object_at("rect", 0, 0, 25, &catalog));
        let rectangle = workspace
            .selected_clip_document()
            .expect("new rectangle becomes the primary selection");
        assert_eq!(
            (rectangle.start, rectangle.duration, rectangle.layer),
            (20, 25, 0)
        );
        assert_eq!(rectangle.effects[0].id, "transform");
        assert_eq!(rectangle.effects[1].id, "rect");
        assert_eq!(
            workspace.status(),
            format!(
                "Added object {}",
                catalog
                    .find("rect")
                    .expect("rectangle metadata exists")
                    .name
            )
        );

        assert!(workspace.undo());
        assert_eq!(workspace.document().clips.len(), 3);
        assert!(workspace.insert_catalog_object_at("scene", 10, 4, 30, &catalog));
        let scene = workspace
            .selected_clip_document()
            .expect("new scene object becomes the primary selection");
        assert_eq!((scene.start, scene.duration, scene.layer), (10, 30, 4));
        assert_eq!(scene.effects[0].id, "transform");
        assert_eq!(scene.effects[1].id, "scene");
        assert_eq!(scene.effects[1].params["targetSceneId"], 2);
    }

    #[test]
    fn drag_preparation_preserves_selected_groups_and_selects_new_anchors() {
        let mut workspace = workspace();
        workspace.click_clip(1, false);
        workspace.click_clip(2, true);
        workspace.prepare_clip_drag(1, false);
        assert_eq!(workspace.selected_clip_ids(), [2, 1]);

        workspace.click_clip(1, false);
        workspace.prepare_clip_drag(2, false);
        assert_eq!(workspace.selected_clip_ids(), [2]);

        workspace.click_clip(1, false);
        workspace.prepare_clip_drag(2, true);
        assert_eq!(workspace.selected_clip_ids(), [2, 1]);
    }

    #[test]
    fn split_selected_clips_is_one_undo_step() {
        let mut workspace = workspace();
        workspace.click_clip(1, false);
        assert!(workspace.split_selected_clips_at(10));
        assert_eq!(workspace.document().clips.len(), 4);
        assert!(workspace.undo());
        assert_eq!(workspace.document().clips.len(), 3);
    }

    #[test]
    fn clip_context_commands_use_the_right_click_frame_and_layer() {
        let mut workspace = workspace();
        // Right-click frame inside clip 1 (0..20), away from the playhead.
        workspace.seek(0);
        workspace.click_clip(2, false);
        workspace.context_click_clip(1);
        assert_eq!(workspace.selected_clip_ids(), [1]);
        assert!(workspace.split_selected_clips_at(10));
        let clip_one: Vec<_> = workspace
            .document()
            .clips
            .iter()
            .filter(|clip| {
                clip.scene_id == workspace.selected_scene() && clip.start < 20 && clip.layer == 0
            })
            .collect();
        assert_eq!(clip_one.len(), 2);
        assert!(workspace.undo());

        // Duplicate pastes at the right-clicked frame/layer like Qt.
        workspace.context_click_clip(1);
        let (next_frame, layer) = workspace
            .duplicate_selected_clips_at(80, 4, 128)
            .expect("duplicate succeeds");
        assert_eq!((next_frame, layer), (100, 4));
        assert!(
            workspace
                .document()
                .clips
                .iter()
                .any(|clip| { clip.start == 80 && clip.layer == 4 && clip.duration == 20 })
        );
    }

    #[test]
    fn layer_visibility_and_lock_are_authoritative_project_edits() {
        let mut workspace = workspace();
        assert!(workspace.toggle_layer_visibility(3));
        assert_eq!(
            workspace.selected_scene_document().unwrap().hidden_layers,
            [3]
        );
        assert!(workspace.toggle_layer_lock(3));
        assert_eq!(
            workspace.selected_scene_document().unwrap().locked_layers,
            [3]
        );
        assert!(workspace.project().dirty);
    }

    #[test]
    fn project_settings_are_clamped_and_stay_out_of_the_qt_undo_stack() {
        let mut workspace = workspace();
        assert!(workspace.update_project_settings(ProjectSettingsInput {
            width: 12_000,
            height: 0,
            fps: f64::NAN,
            sample_rate: 500_000,
        }));
        assert_eq!(workspace.document().settings.width, 8_000);
        assert_eq!(workspace.document().settings.height, 1);
        assert_eq!(workspace.document().settings.fps, 60.0);
        assert_eq!(workspace.document().settings.sample_rate, 192_000);
        assert_eq!(workspace.document_revision(), 1);
        assert!(!workspace.can_undo());
        assert!(!workspace.undo());
        assert_eq!(workspace.document().settings.width, 8_000);
        assert_eq!(workspace.document().settings.height, 1);
    }

    #[test]
    fn scene_creation_and_updates_preserve_the_qt_settings_contract() {
        let mut workspace = workspace();
        let scene_id = workspace
            .create_scene(
                ProjectDefaults {
                    width: 640,
                    height: 480,
                    fps: 24.0,
                    ..ProjectDefaults::default()
                },
                SceneSettingsInput {
                    name: "Configured".to_owned(),
                    width: 1_280,
                    height: 720,
                    fps: 30.0,
                    duration: 900,
                    grid_mode: "BPM".to_owned(),
                    grid_bpm: 128.0,
                    grid_offset: 0.25,
                    grid_interval: 12,
                    grid_subdivision: 8,
                    enable_snap: false,
                    magnetic_snap_range: 18,
                },
            )
            .expect("scene creates");
        assert_eq!(workspace.selected_scene(), scene_id);
        let created = workspace.scene_settings(scene_id).expect("scene exists");
        assert_eq!(created.name, "Configured");
        assert_eq!(created.grid_mode, "BPM");
        assert_eq!(created.grid_subdivision, 8);
        assert!(!created.enable_snap);

        assert!(workspace.update_scene_settings(
            scene_id,
            SceneSettingsInput {
                name: "Updated".to_owned(),
                width: 0,
                height: 40_000,
                fps: f64::INFINITY,
                duration: 0,
                grid_mode: "unknown".to_owned(),
                grid_bpm: 0.0,
                grid_offset: -4.0,
                grid_interval: 0,
                grid_subdivision: 0,
                enable_snap: true,
                magnetic_snap_range: 0,
            }
        ));
        let updated = workspace.scene_settings(scene_id).expect("scene exists");
        assert_eq!(updated.name, "Updated");
        assert_eq!(updated.width, 1_920);
        assert_eq!(updated.height, 1_080);
        assert_eq!(updated.fps, 60.0);
        assert_eq!(updated.duration, 300);
        assert_eq!(updated.grid_mode, "Auto");
        assert_eq!(updated.grid_bpm, 120.0);
        assert_eq!(updated.grid_offset, 0.0);
        assert_eq!(updated.grid_interval, 10);
        assert_eq!(updated.grid_subdivision, 4);
        assert_eq!(updated.magnetic_snap_range, 10);
    }

    #[test]
    fn scene_creation_keeps_qt_add_then_settings_undo_order() {
        let mut workspace = workspace();
        let scene_id = workspace
            .create_scene(
                ProjectDefaults {
                    width: 640,
                    height: 480,
                    fps: 24.0,
                    ..ProjectDefaults::default()
                },
                SceneSettingsInput {
                    name: String::new(),
                    width: 1_280,
                    height: 720,
                    fps: 30.0,
                    duration: 900,
                    grid_mode: "BPM".to_owned(),
                    grid_bpm: 128.0,
                    grid_offset: 0.25,
                    grid_interval: 12,
                    grid_subdivision: 8,
                    enable_snap: false,
                    magnetic_snap_range: 18,
                },
            )
            .expect("scene creates");
        assert_eq!(workspace.scene_settings(scene_id).unwrap().name, "");

        assert!(workspace.undo());
        let initial = workspace.scene_settings(scene_id).expect("scene remains");
        assert_eq!(initial.name, "");
        assert_eq!(initial.width, 640);
        assert_eq!(initial.height, 480);
        assert_eq!(initial.fps, 24.0);
        assert_eq!(initial.duration, 300);
        assert_eq!(initial.grid_mode, "Auto");

        assert!(workspace.undo());
        assert!(workspace.scene_settings(scene_id).is_none());
    }

    #[test]
    fn export_pause_stops_transport_without_moving_the_playhead() {
        let mut workspace = workspace();
        workspace.seek(42);
        workspace.toggle_playback();
        assert!(workspace.is_playing());

        workspace.pause_playback();

        assert!(!workspace.is_playing());
        assert_eq!(workspace.playhead(), 42);
    }

    #[test]
    fn advancing_the_edit_target_preserves_clip_selection() {
        let mut workspace = workspace();
        workspace.click_clip(1, false);

        workspace.set_edit_target(40, 7);

        assert_eq!(workspace.playhead(), 40);
        assert_eq!(workspace.selected_layer(), 7);
        assert_eq!(workspace.selected_clip_ids(), [1]);

        // HEAD clamps seek-family calls to the clip end (50 here) so the
        // edit target can no longer run past the timeline like Qt's
        // unclamped setCurrentFrame_seek does.
        workspace.set_edit_target(80, 7);

        assert_eq!(workspace.playhead(), 50);
        assert_eq!(workspace.selected_layer(), 7);
        assert_eq!(workspace.selected_clip_ids(), [1]);
    }

    #[test]
    fn skimmer_uses_scene_snapping_and_shift_bypasses_it() {
        let mut workspace = workspace();
        let scene_id = workspace.selected_scene();
        let mut settings = workspace.scene_settings(scene_id).expect("scene exists");
        settings.grid_mode = "Frame".to_owned();
        settings.grid_interval = 10;
        settings.enable_snap = true;
        assert!(workspace.update_scene_settings(scene_id, settings));

        assert_eq!(workspace.snap_timeline_frame(16.0, false, 1.0), 20);
        assert_eq!(workspace.snap_timeline_frame(16.0, true, 1.0), 16);
    }

    #[test]
    fn object_settings_commands_follow_selection_and_preserve_typed_values() {
        let mut workspace = workspace_with_effects();
        workspace.click_clip(1, false);
        assert_eq!(workspace.selected_effect_index(), Some(0));
        assert!(workspace.effect_is_selected(0));

        assert!(workspace.select_effect(1, false, false));
        assert_eq!(workspace.selected_effect_index(), Some(1));
        assert!(workspace.context_select_effect(0));
        assert_eq!(workspace.selected_effect_index(), Some(0));
        assert!(workspace.select_effect(1, false, false));
        assert!(workspace.set_effect_enabled(1, false));
        assert!(!workspace.document().clips[0].effects[1].enabled);

        assert!(workspace.set_effect_parameter(0, "count", json!(4.6)));
        let count = &workspace.document().clips[0].effects[0].params["count"];
        assert_eq!(count["$aviqtlType"], "int");
        assert_eq!(count["value"], 5);
        assert_eq!(count["extension"], 7);

        workspace.seek(10);
        assert!(workspace.set_effect_parameter(1, "size", json!(15.0)));
        let track = workspace.document().clips[0].effects[1]
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get("size"));
        let points = inspect_keyframe_track(track, &json!(0), 100);
        assert!(
            points
                .iter()
                .any(|point| point.frame == 10 && point.value == json!(15.0))
        );

        assert!(workspace.reorder_effects(1, 2));
        assert_eq!(workspace.document().clips[0].effects[2].id, "blur");
        assert_eq!(workspace.selected_effect_index(), Some(2));

        let (catalog, _) = EffectCatalog::load();
        assert!(workspace.add_effect(&catalog, "fade"));
        assert_eq!(workspace.document().clips[0].effects[3].id, "fade");
        assert_eq!(workspace.selected_effect_index(), Some(3));

        assert!(workspace.remove_effect(2));
        assert_eq!(workspace.document().clips[0].effects.len(), 3);
        assert_eq!(workspace.selected_effect_index(), Some(2));
    }

    #[test]
    fn effect_group_actions_match_the_qt_sidebar_scopes() {
        let mut workspace = workspace_with_effects();
        workspace.click_clip(1, false);
        assert!(workspace.select_effect(1, false, false));
        assert!(workspace.select_effect(2, true, false));

        assert!(workspace.set_effect_enabled(1, false));
        assert!(workspace.document().clips[0].effects[0].enabled);
        assert!(!workspace.document().clips[0].effects[1].enabled);
        assert!(!workspace.document().clips[0].effects[2].enabled);
        assert!(workspace.undo());
        assert!(!workspace.document().clips[0].effects[1].enabled);
        assert!(workspace.document().clips[0].effects[2].enabled);
        assert!(workspace.undo());
        assert!(workspace.document().clips[0].effects[1].enabled);

        assert!(workspace.remove_effect_group(1));
        assert_eq!(workspace.document().clips[0].effects.len(), 1);
        assert_eq!(workspace.document().clips[0].effects[0].id, "rect");
        assert!(workspace.undo());
        assert_eq!(workspace.document().clips[0].effects.len(), 3);

        assert!(workspace.select_effect(0, false, false));
        assert!(workspace.select_effect(2, true, false));
        assert!(workspace.remove_effect(2));
        assert_eq!(workspace.document().clips[0].effects.len(), 2);
        assert_eq!(workspace.document().clips[0].effects[0].id, "rect");
        assert_eq!(workspace.document().clips[0].effects[1].id, "blur");
    }

    #[test]
    fn selected_effect_delete_keeps_the_leading_transform() {
        let state = TimelineState::from_json(
            br#"{
                "version": 3,
                "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
                "scenes": [{"id": 1, "name": "Root", "duration": 300}],
                "clips": [{
                    "id": 1,
                    "sceneId": 1,
                    "type": "video",
                    "start": 0,
                    "duration": 100,
                    "layer": 0,
                    "effects": [
                        {"id": "transform", "name": "Transform"},
                        {"id": "blur", "name": "Blur"},
                        {"id": "mosaic", "name": "Mosaic"}
                    ]
                }]
            }"#,
        )
        .expect("transform fixture loads");
        let document = state.snapshot();
        let mut workspace = WorkspaceModel::new(ProjectSession {
            state,
            document,
            path: None,
            dirty: false,
        });
        workspace.click_clip(1, false);
        assert!(workspace.select_effect(0, false, false));
        assert!(workspace.select_effect(1, true, false));
        assert!(workspace.select_effect(2, true, false));

        assert!(workspace.remove_selected_effects());
        assert_eq!(workspace.document().clips[0].effects.len(), 1);
        assert_eq!(workspace.document().clips[0].effects[0].id, "transform");
    }

    #[test]
    fn effect_keyframe_actions_reuse_the_normalized_track_contract() {
        let mut workspace = workspace_with_effects();
        workspace.click_clip(1, false);

        assert_eq!(workspace.snap_effect_keyframe_frame(56.0, 1.0, true), 60);
        assert_eq!(workspace.snap_effect_keyframe_frame(56.0, 1.0, false), 56);
        workspace.seek_effect_frame(40);
        assert_eq!(workspace.playhead(), 40);
        assert!(workspace.add_effect_keyframe(1, "size", 50));
        assert!(workspace.set_effect_parameter_at_frame(1, "size", 50, json!(25.0)));
        assert!(workspace.move_effect_keyframe(1, "size", 50, 60));
        assert!(!workspace.move_effect_keyframe(1, "size", 0, 10));
        assert!(workspace.remove_effect_keyframe(1, "size", 20));
        assert!(!workspace.remove_effect_keyframe(1, "size", 0));
        let easing_point = workspace
            .prepare_effect_easing(1, "size", 0, 60)
            .expect("easing endpoints are materialized");
        assert_eq!(easing_point.frame, 0);
        assert!(workspace.set_effect_keyframe_options(
            1,
            "size",
            0,
            json!({"interp":"random","modeParams":{"stepFrames":3}}),
        ));

        let effect = &workspace.document().clips[0].effects[1];
        let track = effect
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get("size"));
        let points = inspect_keyframe_track(track, &effect.params["size"], 100);
        assert_eq!(
            points.iter().map(|point| point.frame).collect::<Vec<_>>(),
            [0, 60, 100]
        );
        assert_eq!(points[1].value, json!(25.0));
        assert_eq!(points[0].interpolation, "random");
        assert_eq!(points[0].options["modeParams"]["stepFrames"], 3);
        assert_eq!(points[2].interpolation, "none");
    }

    #[test]
    fn audio_plugin_stack_parameters_and_keyframes_follow_the_qt_commands() {
        let mut workspace = workspace_with_audio_plugins();
        workspace.click_clip(9, false);
        assert!(workspace.object_settings_uses_audio_plugins());
        assert_eq!(workspace.selected_effect_index(), Some(0));

        assert!(workspace.select_effect(0, false, false));
        assert!(workspace.select_effect(1, true, false));
        assert!(workspace.set_audio_plugin_enabled(0, false));
        assert!(
            workspace.document().clips[0]
                .audio_plugins
                .iter()
                .all(|plugin| !plugin.enabled)
        );

        assert!(workspace.reorder_audio_plugins(0, 1));
        assert_eq!(workspace.document().clips[0].audio_plugins[0].id, "delay");
        assert_eq!(workspace.document().clips[0].audio_plugins[1].id, "gain");

        assert!(workspace.set_audio_plugin_parameter_at_frame(1, "0", 40, json!(0.75)));
        assert_eq!(
            workspace.document().clips[0].audio_plugins[1].params["0"],
            json!(0.75)
        );
        assert!(workspace.seed_audio_plugin_keyframes(1, "0"));
        assert!(!workspace.seed_audio_plugin_keyframes(1, "0"));
        assert!(workspace.set_audio_plugin_parameter_at_frame(1, "0", 40, json!(0.9)));
        assert!(workspace.add_audio_plugin_keyframe(1, "0", 60));
        assert!(workspace.remove_audio_plugin_keyframe(1, "0", 60));
        assert!(!workspace.remove_audio_plugin_keyframe(1, "0", 0));
        assert!(!workspace.remove_audio_plugin_keyframe(1, "0", 100));

        let plugin = &workspace.document().clips[0].audio_plugins[1];
        let points = inspect_keyframe_track(
            plugin.keyframes.as_ref().and_then(|tracks| tracks.get("0")),
            &plugin.params["0"],
            100,
        );
        assert_eq!(
            points.iter().map(|point| point.frame).collect::<Vec<_>>(),
            [0, 40, 100]
        );
        assert_eq!(points[1].value, json!(0.9));
    }

    #[test]
    fn effect_preset_commands_preserve_qt_parameter_keyframe_and_enabled_order() {
        let workspace = workspace_with_effects();
        let clip = &workspace.document().clips[0];
        let preset = EffectPreset {
            version: 1,
            effect_id: "blur".to_owned(),
            name: "Soft".to_owned(),
            enabled: false,
            params: json!({"size": 12, "quality": 2})
                .as_object()
                .cloned()
                .expect("params"),
            keyframes: json!({
                "size": [
                    {"frame": 0, "value": 2, "interp": "linear"},
                    {"frame": 20, "value": 12, "interp": "easeInOutQuad"}
                ]
            })
            .as_object()
            .cloned()
            .expect("keyframes"),
            extra: Default::default(),
        };

        let commands = effect_preset_commands(clip, 1, &preset).expect("preset plans");
        assert!(matches!(
            commands.first(),
            Some(TimelineCommand::SetEffectParameter {
                effect_index: 1,
                ..
            })
        ));
        assert!(commands.iter().any(|command| matches!(
            command,
            TimelineCommand::SetEffectKeyframe {
                effect_index: 1,
                param_name,
                frame: 20,
                options,
                ..
            } if param_name == "size" && options["interp"] == "easeInOutQuad"
        )));
        assert!(matches!(
            commands.last(),
            Some(TimelineCommand::SetEffectEnabled {
                effect_index: 1,
                enabled: false,
                ..
            })
        ));

        let mut stale = preset;
        stale.effect_id = "mosaic".to_owned();
        assert!(effect_preset_commands(clip, 1, &stale).is_err());
    }

    #[test]
    fn workspace_preset_actions_round_trip_the_selected_effect() {
        let root = std::env::temp_dir().join(format!(
            "aviqtl-workspace-preset-{}-{}",
            std::process::id(),
            crate::recovery::generate_recovery_id()
        ));
        let store = PresetStore::from_root(root.clone());
        let mut workspace = workspace_with_effects();
        workspace.click_clip(1, false);
        assert!(workspace.set_effect_parameter(0, "count", json!(5.0)));
        assert!(workspace.save_effect_preset(&store, 0, "Current"));
        assert_eq!(store.names("rect"), ["Current"]);

        assert!(workspace.set_effect_parameter(0, "count", json!(2.0)));
        assert_eq!(
            workspace.document().clips[0].effects[0].params["count"]["value"],
            2
        );
        assert!(workspace.load_effect_preset(&store, 0, "Current"));
        assert_eq!(
            workspace.document().clips[0].effects[0].params["count"]["value"],
            5
        );

        assert!(workspace.delete_effect_preset(&store, 0, "Current"));
        assert!(store.names("rect").is_empty());
        std::fs::remove_dir_all(root).expect("remove preset fixture");
    }

    #[test]
    fn workspace_preset_actions_round_trip_the_selected_audio_plugin() {
        let root = std::env::temp_dir().join(format!(
            "aviqtl-audio-plugin-preset-{}-{}",
            std::process::id(),
            crate::recovery::generate_recovery_id()
        ));
        let store = PresetStore::from_root(root.clone());
        let mut workspace = workspace_with_audio_plugins();
        workspace.click_clip(9, false);
        assert!(workspace.set_audio_plugin_parameter_at_frame(0, "0", 0, json!(0.8)));
        assert!(workspace.save_audio_plugin_preset(&store, 0, "Current"));
        assert_eq!(store.names("gain"), ["Current"]);

        assert!(workspace.set_audio_plugin_parameter_at_frame(0, "0", 0, json!(0.1)));
        assert_eq!(
            workspace.document().clips[0].audio_plugins[0].params["0"],
            json!(0.1)
        );
        assert!(workspace.load_audio_plugin_preset(&store, 0, "Current"));
        assert_eq!(
            workspace.document().clips[0].audio_plugins[0].params["0"],
            json!(0.8)
        );

        assert!(workspace.delete_audio_plugin_preset(&store, 0, "Current"));
        assert!(store.names("gain").is_empty());
        std::fs::remove_dir_all(root).expect("remove preset fixture");
    }
}
