use crate::effect_catalog::EffectCatalog;
use crate::effect_selection::EffectSelection;
use crate::object_settings::{ObjectSettings, project_object_settings, replace_value_payload};
use crate::project_io::{ProjectDefaults, ProjectSession};
use crate::selection::{ClipSelection, SelectionBox};
use crate::timeline_interaction::{TimelineDragRequest, plan_timeline_drag};
use crate::transport::Transport;
use aviqtl_rust_core::api::{
    ClipDocument, ProjectDocument, ProjectSettings, SceneDocument, TimelineCommand,
    TimelineTransaction, clipboard_duration, inspect_keyframe_track, plan_clip_delta_move,
    plan_clipboard_paste, plan_scene_layer_insertion, plan_scene_layer_shift, snap_scene_frame,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Instant;

const DEFAULT_UNDO_LIMIT: usize = 32;

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
                selected: self.selection.is_visually_selected(clip.id),
                primary: self.selection.primary() == Some(clip.id),
            })
            .collect()
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
                .filter(|clip| !clip.effects.is_empty())
                .map(|_| 0)
        })
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
        if index >= clip.effects.len() {
            return false;
        }
        self.effect_selection.click(index, control, shift);
        true
    }

    pub fn context_select_effect(&mut self, index: usize) -> bool {
        let Some(clip) = self.selected_clip_document() else {
            return false;
        };
        if index >= clip.effects.len() {
            return false;
        }
        self.effect_selection.context_click(index);
        true
    }

    pub fn set_effect_enabled(&mut self, effect_index: usize, enabled: bool) -> bool {
        let Some(clip_id) = self
            .selected_clip_document()
            .and_then(|clip| clip.effects.get(effect_index).map(|_| clip.id))
        else {
            return false;
        };
        self.execute(TimelineCommand::SetEffectEnabled {
            clip_id,
            effect_index,
            enabled,
        })
    }

    pub fn remove_effect(&mut self, effect_index: usize) -> bool {
        let Some(clip_id) = self
            .selected_clip_document()
            .and_then(|clip| clip.effects.get(effect_index).map(|_| clip.id))
        else {
            return false;
        };
        if self.execute(TimelineCommand::RemoveEffects {
            clip_id,
            effect_indices: vec![effect_index],
        }) {
            self.effect_selection.apply_removals(&[effect_index]);
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
        let Some((clip, effect, original)) = self.selected_clip_document().and_then(|clip| {
            clip.effects.get(effect_index).and_then(|effect| {
                effect
                    .params
                    .get(param_name)
                    .map(|original| (clip.clone(), effect.clone(), original.clone()))
            })
        }) else {
            return false;
        };
        let value = replace_value_payload(&original, value);
        let relative_frame = self
            .playhead
            .saturating_sub(clip.start)
            .clamp(0, clip.duration.max(0));
        let track = effect
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param_name));
        let command = if track.is_some() {
            let options = inspect_keyframe_track(track, &original, clip.duration)
                .into_iter()
                .find(|point| point.frame == relative_frame)
                .map_or_else(|| json!({"interp": "linear"}), |point| point.options);
            TimelineCommand::SetEffectKeyframe {
                clip_id: clip.id,
                effect_index,
                param_name: param_name.to_owned(),
                frame: relative_frame,
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
    ) -> Option<(i32, i32)> {
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
            requested_layer.clamp(0, 127),
        ))
    }

    pub fn duplicate_selected_clips_at(&mut self, frame: i32, layer: i32) -> Option<(i32, i32)> {
        self.copy_selected_clips();
        self.paste_clips_at(frame, layer)
    }

    pub fn move_selected_clips(&mut self, delta_layer: i32, delta_frame: i32) -> bool {
        let ids = self.selection.ids().to_vec();
        if ids.is_empty() {
            return false;
        }
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

    pub fn set_all_layers_visible(&mut self, visible: bool) -> bool {
        let Some(mut scene) = self.selected_scene_document().cloned() else {
            return false;
        };
        scene.hidden_layers = if visible {
            Vec::new()
        } else {
            (0..128).collect()
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

    pub fn insert_layers(&mut self, target_layer: i32, count: i32, above: bool) -> bool {
        match plan_scene_layer_insertion(
            &self.project.document,
            self.selected_scene,
            target_layer,
            count,
            above,
        ) {
            Ok(updates) => self.execute_geometry_updates(updates),
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn shift_layers(&mut self, start_layer: i32, end_layer: i32, delta: i32) -> bool {
        match plan_scene_layer_shift(
            &self.project.document,
            self.selected_scene,
            start_layer,
            end_layer,
            delta,
        ) {
            Ok(updates) => self.execute_geometry_updates(updates),
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    pub fn seek(&mut self, frame: i32) {
        self.playhead = frame.clamp(0, self.scene_timing().1.max(0));
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
        let end_frame = self.scene_timing().1;
        self.transport
            .toggle(Instant::now(), &mut self.playhead, end_frame);
    }

    pub fn step_playhead(&mut self, delta: i32) {
        let end_frame = self.scene_timing().1;
        self.transport.step(delta, &mut self.playhead, end_frame);
    }

    pub fn begin_scrub(&mut self) {
        self.transport.begin_scrub();
    }

    pub fn scrub_to(&mut self, frame: i32) -> bool {
        let end_frame = self.scene_timing().1;
        self.transport.scrub_to(
            Instant::now(),
            frame.clamp(0, end_frame.max(0)),
            &mut self.playhead,
        )
    }

    pub fn end_scrub(&mut self) {
        self.transport.end_scrub(Instant::now(), self.playhead);
    }

    pub fn update_transport(&mut self, now: Instant) -> bool {
        let (fps, end_frame) = self.scene_timing();
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
        let end_frame = self.scene_timing().1.max(0);
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
        self.effect_selection.reconcile(clip.id, clip.effects.len());
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

    #[test]
    fn scene_and_clip_snapshots_follow_the_active_workspace() {
        let mut workspace = workspace();
        assert_eq!(workspace.scene_tabs().len(), 2);
        assert_eq!(workspace.timeline_clips().len(), 2);
        assert!(workspace.switch_scene(2));
        assert_eq!(workspace.timeline_clips().len(), 1);
        assert_eq!(workspace.timeline_clips()[0].id, 3);
        assert_eq!(workspace.playhead(), 0);
    }

    #[test]
    fn selection_clipboard_and_history_match_the_qt_command_order() {
        let mut workspace = workspace();
        assert_eq!(workspace.document_revision(), 0);
        workspace.click_clip(1, false);
        workspace.click_clip(2, true);
        assert_eq!(workspace.selected_clip_ids(), [2, 1]);
        assert!(workspace.copy_selected_clips());
        assert_eq!(workspace.paste_clips_at(80, 4), Some((130, 4)));
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

        workspace.set_edit_target(80, 7);

        assert_eq!(workspace.playhead(), 80);
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

        assert!(workspace.remove_effect(1));
        assert_eq!(workspace.document().clips[0].effects.len(), 1);
        assert_eq!(workspace.selected_effect_index(), Some(0));
    }
}
