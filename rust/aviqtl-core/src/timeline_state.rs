use crate::abi::{
    AviQtlIdAllocation, STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON,
    STATUS_OK, STATUS_OVERLAPPING_BUFFERS, STATUS_STATE_CONFLICT, STATUS_UNSUPPORTED_VERSION,
    ranges_overlap, slice_is_valid,
};
use crate::keyframe_document::{split_track, sync_track};
use crate::project::{
    AudioPluginDocument, ClipDocument, EffectDocument, ProjectDocument, ProjectError,
    SceneDocument, parse_project_document,
};
use crate::timeline_domain::allocate_id;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::sync::Mutex;

type OptionalTracks = Option<Map<String, Value>>;
type SplitTracks = (OptionalTracks, OptionalTracks);

#[derive(Debug, Clone)]
struct TimelineState {
    document: ProjectDocument,
    next_clip_id: i32,
    next_scene_id: i32,
}

impl TimelineState {
    fn new(
        document: ProjectDocument,
        next_clip_hint: i32,
        next_scene_hint: i32,
    ) -> Result<Self, StateError> {
        validate_document(&document)?;
        let minimum_clip_id = document
            .clips
            .iter()
            .map(|clip| clip.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        let minimum_scene_id = document
            .scenes
            .iter()
            .map(|scene| scene.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        Ok(Self {
            document,
            next_clip_id: next_clip_hint.max(minimum_clip_id),
            next_scene_id: next_scene_hint.max(minimum_scene_id),
        })
    }

    fn reserve_ids(&mut self, kind: EntityKind, count: usize) -> Result<Vec<i32>, StateError> {
        let (mut existing, mut next_hint) = match kind {
            EntityKind::Clip => (
                self.document
                    .clips
                    .iter()
                    .map(|clip| clip.id)
                    .collect::<Vec<_>>(),
                self.next_clip_id,
            ),
            EntityKind::Scene => (
                self.document
                    .scenes
                    .iter()
                    .map(|scene| scene.id)
                    .collect::<Vec<_>>(),
                self.next_scene_id,
            ),
        };
        let mut allocated = Vec::with_capacity(count);
        for _ in 0..count {
            let AviQtlIdAllocation {
                allocated_id,
                next_id,
            } = allocate_id(&existing, next_hint, 1).ok_or(StateError::InvalidArgument)?;
            existing.push(allocated_id);
            allocated.push(allocated_id);
            next_hint = next_id;
        }
        match kind {
            EntityKind::Clip => self.next_clip_id = next_hint,
            EntityKind::Scene => self.next_scene_id = next_hint,
        }
        Ok(allocated)
    }

    fn plan(&self, request: EditRequest) -> Result<Transaction, StateError> {
        match request {
            EditRequest::InsertScene { index, scene } => {
                if self.document.scenes.iter().any(|item| item.id == scene.id) {
                    return Err(StateError::Conflict);
                }
                let index = index.unwrap_or(self.document.scenes.len());
                if index > self.document.scenes.len() {
                    return Err(StateError::InvalidArgument);
                }
                Ok(transaction(
                    vec![PatchOperation::InsertScene {
                        index,
                        scene: scene.clone(),
                    }],
                    vec![PatchOperation::RemoveScene { index, scene }],
                ))
            }
            EditRequest::RemoveScene { scene_id } => {
                let scene_index = self
                    .document
                    .scenes
                    .iter()
                    .position(|scene| scene.id == scene_id)
                    .ok_or(StateError::InvalidArgument)?;
                let scene = self.document.scenes[scene_index].clone();
                let clips = self
                    .document
                    .clips
                    .iter()
                    .enumerate()
                    .filter(|(_, clip)| clip.scene_id == scene_id)
                    .map(|(index, clip)| (index, clip.clone()))
                    .collect::<Vec<_>>();

                let mut forward = clips
                    .iter()
                    .rev()
                    .map(|(index, clip)| PatchOperation::RemoveClip {
                        index: *index,
                        clip: clip.clone(),
                    })
                    .collect::<Vec<_>>();
                forward.push(PatchOperation::RemoveScene {
                    index: scene_index,
                    scene: scene.clone(),
                });

                let mut inverse = vec![PatchOperation::InsertScene {
                    index: scene_index,
                    scene,
                }];
                inverse.extend(
                    clips
                        .into_iter()
                        .map(|(index, clip)| PatchOperation::InsertClip { index, clip }),
                );
                Ok(transaction(forward, inverse))
            }
            EditRequest::ReplaceScene { scene_id, scene } => {
                if scene.id != scene_id {
                    return Err(StateError::InvalidArgument);
                }
                let index = self
                    .document
                    .scenes
                    .iter()
                    .position(|item| item.id == scene_id)
                    .ok_or(StateError::InvalidArgument)?;
                let before = self.document.scenes[index].clone();
                Ok(replacement_transaction(
                    PatchOperation::ReplaceScene {
                        index,
                        before: before.clone(),
                        after: scene.clone(),
                    },
                    PatchOperation::ReplaceScene {
                        index,
                        before: scene,
                        after: before,
                    },
                ))
            }
            EditRequest::UpdateScene {
                scene_id,
                mut scene,
            } => {
                if scene.id != scene_id {
                    return Err(StateError::InvalidArgument);
                }
                let index = self
                    .document
                    .scenes
                    .iter()
                    .position(|item| item.id == scene_id)
                    .ok_or(StateError::InvalidArgument)?;
                let before = self.document.scenes[index].clone();
                scene.extra = before.extra.clone();
                Ok(replacement_transaction(
                    PatchOperation::ReplaceScene {
                        index,
                        before: before.clone(),
                        after: scene.clone(),
                    },
                    PatchOperation::ReplaceScene {
                        index,
                        before: scene,
                        after: before,
                    },
                ))
            }
            EditRequest::InsertClip { index, clip } => {
                if self.document.clips.iter().any(|item| item.id == clip.id)
                    || !self
                        .document
                        .scenes
                        .iter()
                        .any(|scene| scene.id == clip.scene_id)
                {
                    return Err(StateError::Conflict);
                }
                let index =
                    index.unwrap_or_else(|| clip_insert_index(&self.document, clip.scene_id));
                if index > self.document.clips.len() {
                    return Err(StateError::InvalidArgument);
                }
                Ok(transaction(
                    vec![PatchOperation::InsertClip {
                        index,
                        clip: clip.clone(),
                    }],
                    vec![PatchOperation::RemoveClip { index, clip }],
                ))
            }
            EditRequest::RemoveClip { clip_id } => {
                let index = self
                    .document
                    .clips
                    .iter()
                    .position(|clip| clip.id == clip_id)
                    .ok_or(StateError::InvalidArgument)?;
                let clip = self.document.clips[index].clone();
                Ok(transaction(
                    vec![PatchOperation::RemoveClip {
                        index,
                        clip: clip.clone(),
                    }],
                    vec![PatchOperation::InsertClip { index, clip }],
                ))
            }
            EditRequest::ReplaceClip { clip_id, clip } => {
                plan_clip_replacements(&self.document, vec![(clip_id, clip)])
            }
            EditRequest::BatchReplaceClips { replacements } => {
                let replacements = replacements
                    .into_iter()
                    .map(|replacement| (replacement.clip_id, replacement.clip))
                    .collect();
                plan_clip_replacements(&self.document, replacements)
            }
            EditRequest::UpdateClipGeometry {
                clip_id,
                layer,
                start,
                duration,
            } => plan_clip_geometry_updates(
                &self.document,
                vec![ClipGeometryUpdate {
                    clip_id,
                    layer,
                    start,
                    duration,
                }],
            ),
            EditRequest::BatchUpdateClipGeometry { updates } => {
                plan_clip_geometry_updates(&self.document, updates)
            }
            EditRequest::SetClipByUpperObject { clip_id, enabled } => {
                let mut clip = self
                    .document
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .cloned()
                    .ok_or(StateError::InvalidArgument)?;
                clip.clip_by_upper_object = enabled;
                plan_clip_replacements(&self.document, vec![(clip_id, clip)])
            }
            EditRequest::InsertEffects {
                clip_id,
                insertions,
            } => {
                if insertions.is_empty() {
                    return Err(StateError::InvalidArgument);
                }
                let mut clip = self
                    .document
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .cloned()
                    .ok_or(StateError::InvalidArgument)?;
                for insertion in insertions {
                    if insertion.index > clip.effects.len() {
                        return Err(StateError::InvalidArgument);
                    }
                    clip.effects.insert(insertion.index, insertion.effect);
                }
                plan_clip_replacements(&self.document, vec![(clip_id, clip)])
            }
            EditRequest::RemoveEffects {
                clip_id,
                mut effect_indices,
            } => {
                if effect_indices.is_empty() {
                    return Err(StateError::InvalidArgument);
                }
                let mut clip = self
                    .document
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .cloned()
                    .ok_or(StateError::InvalidArgument)?;
                effect_indices.sort_unstable();
                if effect_indices
                    .windows(2)
                    .any(|indices| indices[0] == indices[1])
                    || effect_indices
                        .last()
                        .is_none_or(|index| *index >= clip.effects.len())
                    || (clip
                        .effects
                        .first()
                        .is_some_and(|effect| effect.id == "transform")
                        && effect_indices.first() == Some(&0))
                {
                    return Err(StateError::InvalidArgument);
                }
                for index in effect_indices.into_iter().rev() {
                    clip.effects.remove(index);
                }
                plan_clip_replacements(&self.document, vec![(clip_id, clip)])
            }
            EditRequest::ReorderEffects {
                clip_id,
                permutation,
            } => {
                let mut clip = self
                    .document
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .cloned()
                    .ok_or(StateError::InvalidArgument)?;
                if permutation.len() != clip.effects.len()
                    || permutation.iter().any(|index| *index >= clip.effects.len())
                    || permutation.iter().copied().collect::<BTreeSet<_>>().len()
                        != permutation.len()
                    || (clip
                        .effects
                        .first()
                        .is_some_and(|effect| effect.id == "transform")
                        && permutation.first() != Some(&0))
                {
                    return Err(StateError::InvalidArgument);
                }
                let reordered = permutation
                    .into_iter()
                    .map(|index| clip.effects[index].clone())
                    .collect();
                clip.effects = reordered;
                plan_clip_replacements(&self.document, vec![(clip_id, clip)])
            }
            EditRequest::SetEffectEnabled {
                clip_id,
                effect_index,
                enabled,
            } => {
                let mut clip = self
                    .document
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .cloned()
                    .ok_or(StateError::InvalidArgument)?;
                let effect = clip
                    .effects
                    .get_mut(effect_index)
                    .ok_or(StateError::InvalidArgument)?;
                effect.enabled = enabled;
                plan_clip_replacements(&self.document, vec![(clip_id, clip)])
            }
            EditRequest::SplitClip {
                clip_id,
                frame,
                new_clip_id,
            } => self.plan_split(clip_id, frame, new_clip_id),
            EditRequest::ReplaceDocument { document } => {
                validate_document(&document)?;
                Ok(plan_document_replacement(&self.document, document))
            }
        }
    }

    fn plan_split(
        &self,
        clip_id: i32,
        frame: i32,
        new_clip_id: i32,
    ) -> Result<Transaction, StateError> {
        if new_clip_id < 1
            || self
                .document
                .clips
                .iter()
                .any(|clip| clip.id == new_clip_id)
        {
            return Err(StateError::Conflict);
        }
        let index = self
            .document
            .clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or(StateError::InvalidArgument)?;
        let before = self.document.clips[index].clone();
        let end = i64::from(before.start) + i64::from(before.duration);
        if i64::from(frame) <= i64::from(before.start) || i64::from(frame) >= end {
            return Err(StateError::InvalidArgument);
        }
        let first_duration = frame.saturating_sub(before.start);
        let second_duration = before.duration.saturating_sub(first_duration);
        let mut first = before.clone();
        let mut second = before.clone();
        first.duration = first_duration;
        second.id = new_clip_id;
        second.start = frame;
        second.duration = second_duration;
        split_clip_tracks(&mut first, &mut second, first_duration, before.duration)?;

        Ok(transaction(
            vec![
                PatchOperation::ReplaceClip {
                    index,
                    before: before.clone(),
                    after: first.clone(),
                },
                PatchOperation::InsertClip {
                    index: index + 1,
                    clip: second.clone(),
                },
            ],
            vec![
                PatchOperation::RemoveClip {
                    index: index + 1,
                    clip: second,
                },
                PatchOperation::ReplaceClip {
                    index,
                    before: first,
                    after: before,
                },
            ],
        ))
    }

    fn apply_patch(&mut self, patch: &Patch) -> Result<(), StateError> {
        let mut candidate = self.document.clone();
        for operation in &patch.operations {
            apply_operation(&mut candidate, operation)?;
        }
        validate_document(&candidate)?;
        self.document = candidate;
        Ok(())
    }
}

pub struct AviQtlTimelineState {
    state: Mutex<TimelineState>,
}

#[derive(Debug, Clone, Copy)]
enum EntityKind {
    Clip,
    Scene,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateError {
    InvalidJson,
    UnsupportedVersion,
    InvalidArgument,
    Conflict,
}

impl From<ProjectError> for StateError {
    fn from(error: ProjectError) -> Self {
        match error {
            ProjectError::InvalidJson => Self::InvalidJson,
            ProjectError::UnsupportedVersion => Self::UnsupportedVersion,
        }
    }
}

impl StateError {
    const fn status(self) -> u32 {
        match self {
            Self::InvalidJson => STATUS_INVALID_JSON,
            Self::UnsupportedVersion => STATUS_UNSUPPORTED_VERSION,
            Self::InvalidArgument => STATUS_INVALID_ARGUMENT,
            Self::Conflict => STATUS_STATE_CONFLICT,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum EditRequest {
    InsertScene {
        #[serde(default)]
        index: Option<usize>,
        scene: SceneDocument,
    },
    RemoveScene {
        scene_id: i32,
    },
    ReplaceScene {
        scene_id: i32,
        scene: SceneDocument,
    },
    UpdateScene {
        scene_id: i32,
        scene: SceneDocument,
    },
    InsertClip {
        #[serde(default)]
        index: Option<usize>,
        clip: ClipDocument,
    },
    RemoveClip {
        clip_id: i32,
    },
    ReplaceClip {
        clip_id: i32,
        clip: ClipDocument,
    },
    BatchReplaceClips {
        replacements: Vec<ClipReplacement>,
    },
    UpdateClipGeometry {
        clip_id: i32,
        layer: i32,
        start: i32,
        duration: i32,
    },
    BatchUpdateClipGeometry {
        updates: Vec<ClipGeometryUpdate>,
    },
    SetClipByUpperObject {
        clip_id: i32,
        enabled: bool,
    },
    InsertEffects {
        clip_id: i32,
        insertions: Vec<EffectInsertion>,
    },
    RemoveEffects {
        clip_id: i32,
        effect_indices: Vec<usize>,
    },
    ReorderEffects {
        clip_id: i32,
        permutation: Vec<usize>,
    },
    SetEffectEnabled {
        clip_id: i32,
        effect_index: usize,
        enabled: bool,
    },
    SplitClip {
        clip_id: i32,
        frame: i32,
        new_clip_id: i32,
    },
    ReplaceDocument {
        document: ProjectDocument,
    },
}

#[derive(Debug, Deserialize)]
struct ClipReplacement {
    clip_id: i32,
    clip: ClipDocument,
}

#[derive(Debug, Deserialize)]
struct ClipGeometryUpdate {
    clip_id: i32,
    layer: i32,
    start: i32,
    duration: i32,
}

#[derive(Debug, Deserialize)]
struct EffectInsertion {
    index: usize,
    effect: EffectDocument,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Transaction {
    forward: Patch,
    inverse: Patch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Patch {
    operations: Vec<PatchOperation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum PatchOperation {
    InsertScene {
        index: usize,
        scene: SceneDocument,
    },
    RemoveScene {
        index: usize,
        scene: SceneDocument,
    },
    ReplaceScene {
        index: usize,
        before: SceneDocument,
        after: SceneDocument,
    },
    InsertClip {
        index: usize,
        clip: ClipDocument,
    },
    RemoveClip {
        index: usize,
        clip: ClipDocument,
    },
    ReplaceClip {
        index: usize,
        before: ClipDocument,
        after: ClipDocument,
    },
}

fn transaction(forward: Vec<PatchOperation>, inverse: Vec<PatchOperation>) -> Transaction {
    Transaction {
        forward: Patch {
            operations: forward,
        },
        inverse: Patch {
            operations: inverse,
        },
    }
}

fn replacement_transaction(forward: PatchOperation, inverse: PatchOperation) -> Transaction {
    transaction(vec![forward], vec![inverse])
}

/// Plans a scene and clip replacement while preserving the current version, settings, and extras.
/// Use `aviqtl_timeline_state_reset` when those document-level fields must also be replaced.
fn plan_document_replacement(before: &ProjectDocument, mut after: ProjectDocument) -> Transaction {
    after.version = before.version;
    after.settings = before.settings.clone();
    after.extra = before.extra.clone();

    let mut forward = before
        .clips
        .iter()
        .enumerate()
        .rev()
        .map(|(index, clip)| PatchOperation::RemoveClip {
            index,
            clip: clip.clone(),
        })
        .collect::<Vec<_>>();
    forward.extend(
        before
            .scenes
            .iter()
            .enumerate()
            .rev()
            .map(|(index, scene)| PatchOperation::RemoveScene {
                index,
                scene: scene.clone(),
            }),
    );
    forward.extend(
        after
            .scenes
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, scene)| PatchOperation::InsertScene { index, scene }),
    );
    forward.extend(
        after
            .clips
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, clip)| PatchOperation::InsertClip { index, clip }),
    );

    let mut inverse = after
        .clips
        .iter()
        .enumerate()
        .rev()
        .map(|(index, clip)| PatchOperation::RemoveClip {
            index,
            clip: clip.clone(),
        })
        .collect::<Vec<_>>();
    inverse.extend(after.scenes.iter().enumerate().rev().map(|(index, scene)| {
        PatchOperation::RemoveScene {
            index,
            scene: scene.clone(),
        }
    }));
    inverse.extend(
        before
            .scenes
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, scene)| PatchOperation::InsertScene { index, scene }),
    );
    inverse.extend(
        before
            .clips
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, clip)| PatchOperation::InsertClip { index, clip }),
    );
    transaction(forward, inverse)
}

fn clip_insert_index(document: &ProjectDocument, scene_id: i32) -> usize {
    document
        .clips
        .iter()
        .rposition(|clip| clip.scene_id == scene_id)
        .map_or(document.clips.len(), |index| index + 1)
}

fn plan_clip_replacements(
    document: &ProjectDocument,
    replacements: Vec<(i32, ClipDocument)>,
) -> Result<Transaction, StateError> {
    let mut seen = BTreeSet::new();
    let mut indexed = Vec::with_capacity(replacements.len());
    for (clip_id, mut clip) in replacements {
        if clip.id != clip_id || !seen.insert(clip_id) {
            return Err(StateError::InvalidArgument);
        }
        if !document
            .scenes
            .iter()
            .any(|scene| scene.id == clip.scene_id)
        {
            return Err(StateError::Conflict);
        }
        let index = document
            .clips
            .iter()
            .position(|item| item.id == clip_id)
            .ok_or(StateError::InvalidArgument)?;
        if document.clips[index].duration != clip.duration {
            sync_clip_tracks(&mut clip, document.clips[index].duration);
        }
        indexed.push((index, document.clips[index].clone(), clip));
    }
    indexed.sort_by_key(|(index, _, _)| *index);
    let forward = indexed
        .iter()
        .map(|(index, before, after)| PatchOperation::ReplaceClip {
            index: *index,
            before: before.clone(),
            after: after.clone(),
        })
        .collect();
    let inverse = indexed
        .into_iter()
        .rev()
        .map(|(index, before, after)| PatchOperation::ReplaceClip {
            index,
            before: after,
            after: before,
        })
        .collect();
    Ok(transaction(forward, inverse))
}

fn plan_clip_geometry_updates(
    document: &ProjectDocument,
    updates: Vec<ClipGeometryUpdate>,
) -> Result<Transaction, StateError> {
    if updates.is_empty() {
        return Err(StateError::InvalidArgument);
    }
    let replacements = updates
        .into_iter()
        .map(|update| {
            let mut clip = document
                .clips
                .iter()
                .find(|clip| clip.id == update.clip_id)
                .cloned()
                .ok_or(StateError::InvalidArgument)?;
            clip.layer = update.layer;
            clip.start = update.start;
            clip.duration = update.duration;
            Ok((update.clip_id, clip))
        })
        .collect::<Result<Vec<_>, StateError>>()?;
    plan_clip_replacements(document, replacements)
}

fn split_clip_tracks(
    first: &mut ClipDocument,
    second: &mut ClipDocument,
    first_duration: i32,
    original_duration: i32,
) -> Result<(), StateError> {
    for (first_effect, second_effect) in first.effects.iter_mut().zip(&mut second.effects) {
        split_effect_tracks(
            first_effect,
            second_effect,
            first_duration,
            original_duration,
        )?;
    }
    for (first_plugin, second_plugin) in first
        .audio_plugins
        .iter_mut()
        .zip(&mut second.audio_plugins)
    {
        split_audio_plugin_tracks(
            first_plugin,
            second_plugin,
            first_duration,
            original_duration,
        )?;
    }
    Ok(())
}

fn sync_clip_tracks(clip: &mut ClipDocument, old_duration: i32) {
    for effect in &mut clip.effects {
        sync_tracks(
            &mut effect.keyframes,
            &effect.params,
            old_duration,
            clip.duration,
        );
    }
    for plugin in &mut clip.audio_plugins {
        sync_tracks(
            &mut plugin.keyframes,
            &plugin.params,
            old_duration,
            clip.duration,
        );
    }
}

fn sync_tracks(
    tracks: &mut Option<Map<String, Value>>,
    params: &Map<String, Value>,
    old_duration: i32,
    new_duration: i32,
) {
    let Some(current) = tracks else {
        return;
    };
    for (key, fallback) in params {
        let track = current.remove(key).unwrap_or(Value::Null);
        current.insert(
            key.clone(),
            sync_track(track, fallback.clone(), old_duration, new_duration),
        );
    }
}

fn split_effect_tracks(
    first: &mut EffectDocument,
    second: &mut EffectDocument,
    first_duration: i32,
    original_duration: i32,
) -> Result<(), StateError> {
    let (first_tracks, second_tracks) = split_tracks(
        first.keyframes.take(),
        &first.params,
        first_duration,
        original_duration,
    )?;
    first.keyframes = first_tracks;
    second.keyframes = second_tracks;
    Ok(())
}

fn split_audio_plugin_tracks(
    first: &mut AudioPluginDocument,
    second: &mut AudioPluginDocument,
    first_duration: i32,
    original_duration: i32,
) -> Result<(), StateError> {
    let (first_tracks, second_tracks) = split_tracks(
        first.keyframes.take(),
        &first.params,
        first_duration,
        original_duration,
    )?;
    first.keyframes = first_tracks;
    second.keyframes = second_tracks;
    Ok(())
}

fn split_tracks(
    tracks: OptionalTracks,
    params: &Map<String, Value>,
    first_duration: i32,
    original_duration: i32,
) -> Result<SplitTracks, StateError> {
    let Some(tracks) = tracks else {
        return Ok((None, None));
    };
    let mut first_tracks = Map::new();
    let mut second_tracks = Map::new();
    for (key, track) in tracks {
        let fallback = params.get(&key).cloned().unwrap_or(Value::Null);
        let (first, second) = split_track(track, fallback, first_duration, original_duration)
            .ok_or(StateError::InvalidArgument)?;
        first_tracks.insert(key.clone(), first);
        second_tracks.insert(key, second);
    }
    Ok((Some(first_tracks), Some(second_tracks)))
}

fn apply_operation(
    document: &mut ProjectDocument,
    operation: &PatchOperation,
) -> Result<(), StateError> {
    match operation {
        PatchOperation::InsertScene { index, scene } => {
            if *index > document.scenes.len()
                || document.scenes.iter().any(|item| item.id == scene.id)
            {
                return Err(StateError::Conflict);
            }
            document.scenes.insert(*index, scene.clone());
        }
        PatchOperation::RemoveScene { index, scene } => {
            if document.scenes.get(*index) != Some(scene)
                || document.clips.iter().any(|clip| clip.scene_id == scene.id)
            {
                return Err(StateError::Conflict);
            }
            document.scenes.remove(*index);
        }
        PatchOperation::ReplaceScene {
            index,
            before,
            after,
        } => {
            if before.id != after.id || document.scenes.get(*index) != Some(before) {
                return Err(StateError::Conflict);
            }
            document.scenes[*index] = after.clone();
        }
        PatchOperation::InsertClip { index, clip } => {
            if *index > document.clips.len()
                || document.clips.iter().any(|item| item.id == clip.id)
                || !document
                    .scenes
                    .iter()
                    .any(|scene| scene.id == clip.scene_id)
            {
                return Err(StateError::Conflict);
            }
            document.clips.insert(*index, clip.clone());
        }
        PatchOperation::RemoveClip { index, clip } => {
            if document.clips.get(*index) != Some(clip) {
                return Err(StateError::Conflict);
            }
            document.clips.remove(*index);
        }
        PatchOperation::ReplaceClip {
            index,
            before,
            after,
        } => {
            if before.id != after.id
                || document.clips.get(*index) != Some(before)
                || !document
                    .scenes
                    .iter()
                    .any(|scene| scene.id == after.scene_id)
            {
                return Err(StateError::Conflict);
            }
            document.clips[*index] = after.clone();
        }
    }
    Ok(())
}

fn validate_document(document: &ProjectDocument) -> Result<(), StateError> {
    let mut scene_ids = BTreeSet::new();
    if document
        .scenes
        .iter()
        .any(|scene| scene.id < 0 || !scene_ids.insert(scene.id))
    {
        return Err(StateError::InvalidArgument);
    }
    let mut clip_ids = BTreeSet::new();
    if document.clips.iter().any(|clip| {
        clip.id < 1
            || clip.start < 0
            || clip.duration < 1
            || !(0..=127).contains(&clip.layer)
            || !scene_ids.contains(&clip.scene_id)
            || !clip_ids.insert(clip.id)
    }) {
        return Err(StateError::InvalidArgument);
    }
    Ok(())
}

unsafe fn input_bytes<'a>(input: *const u8, input_length: usize) -> Option<&'a [u8]> {
    if !slice_is_valid(input, input_length) {
        return None;
    }
    if input_length == 0 {
        Some(&[])
    } else {
        // SAFETY: The pointer range was validated above and the caller owns its lifetime.
        Some(unsafe { std::slice::from_raw_parts(input, input_length) })
    }
}

unsafe fn write_json<T: Serialize>(
    value: &T,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(output, output_capacity) || !slice_is_valid(output_length, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(output, output_capacity, output_length, 1) {
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        Some(false) => {}
        None => return STATUS_INVALID_ARGUMENT,
    }
    let bytes = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(_) => return STATUS_INVALID_JSON,
    };
    // SAFETY: output_length was validated above.
    unsafe { output_length.write(bytes.len()) };
    if output_capacity < bytes.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !bytes.is_empty() {
        // SAFETY: output covers at least bytes.len() writable bytes and does not overlap length.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    }
    STATUS_OK
}

unsafe fn state_from_handle<'a>(
    handle: *mut AviQtlTimelineState,
) -> Option<&'a AviQtlTimelineState> {
    if handle.is_null() {
        None
    } else {
        // SAFETY: The caller guarantees that handle came from create and has not been destroyed.
        Some(unsafe { &*handle })
    }
}

/// Creates an opaque Rust-owned timeline state from a normalized project document.
///
/// # Safety
///
/// The input must be readable and `output_handle` writable and disjoint from the input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_create(
    input: *const u8,
    input_length: usize,
    next_clip_hint: i32,
    next_scene_hint: i32,
    output_handle: *mut *mut AviQtlTimelineState,
) -> u32 {
    if !slice_is_valid(output_handle, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(input, input_length, output_handle, 1) {
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        Some(false) => {}
        None => return STATUS_INVALID_ARGUMENT,
    }
    // SAFETY: output_handle was validated above.
    unsafe { output_handle.write(std::ptr::null_mut()) };
    // SAFETY: The caller provides the input range for this call.
    let Some(bytes) = (unsafe { input_bytes(input, input_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let document = match parse_project_document(bytes) {
        Ok(document) => document,
        Err(error) => return StateError::from(error).status(),
    };
    let state = match TimelineState::new(document, next_clip_hint, next_scene_hint) {
        Ok(state) => state,
        Err(error) => return error.status(),
    };
    let handle = Box::into_raw(Box::new(AviQtlTimelineState {
        state: Mutex::new(state),
    }));
    // SAFETY: output_handle was validated and is disjoint from input.
    unsafe { output_handle.write(handle) };
    STATUS_OK
}

/// Destroys an opaque timeline state. A null handle is accepted.
///
/// # Safety
///
/// Non-null handles must have been returned by `aviqtl_timeline_state_create` exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_destroy(handle: *mut AviQtlTimelineState) {
    if !handle.is_null() {
        // SAFETY: The caller transfers ownership of a live handle exactly once.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Replaces a timeline state's document while preserving its opaque handle.
///
/// # Safety
///
/// The handle must be live and input readable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_reset(
    handle: *mut AviQtlTimelineState,
    input: *const u8,
    input_length: usize,
    next_clip_hint: i32,
    next_scene_hint: i32,
) -> u32 {
    // SAFETY: The caller upholds the handle and input contracts above.
    let (Some(handle), Some(bytes)) = (unsafe { state_from_handle(handle) }, unsafe {
        input_bytes(input, input_length)
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let document = match parse_project_document(bytes) {
        Ok(document) => document,
        Err(error) => return StateError::from(error).status(),
    };
    let state = match TimelineState::new(document, next_clip_hint, next_scene_hint) {
        Ok(state) => state,
        Err(error) => return error.status(),
    };
    let Ok(mut guard) = handle.state.lock() else {
        return STATUS_INVALID_ARGUMENT;
    };
    *guard = state;
    STATUS_OK
}

/// Serializes the authoritative project/timeline state into caller-owned storage.
///
/// # Safety
///
/// The handle must be live. Output and output length must be writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_snapshot_json(
    handle: *mut AviQtlTimelineState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: The caller upholds the handle contract above.
    let Some(handle) = (unsafe { state_from_handle(handle) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Ok(guard) = handle.state.lock() else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The caller upholds the output contract above.
    unsafe { write_json(&guard.document, output, output_capacity, output_length) }
}

/// Plans a reversible edit without mutating the state.
///
/// # Safety
///
/// The handle must be live, input readable, and output ranges writable and disjoint from input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_plan_json(
    handle: *mut AviQtlTimelineState,
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if ranges_overlap(input, input_length, output, output_capacity) != Some(false)
        || ranges_overlap(input, input_length, output_length, 1) != Some(false)
    {
        return if slice_is_valid(input, input_length)
            && slice_is_valid(output, output_capacity)
            && slice_is_valid(output_length, 1)
        {
            STATUS_OVERLAPPING_BUFFERS
        } else {
            STATUS_INVALID_ARGUMENT
        };
    }
    // SAFETY: The caller upholds the handle and input contracts above.
    let (Some(handle), Some(bytes)) = (unsafe { state_from_handle(handle) }, unsafe {
        input_bytes(input, input_length)
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let request: EditRequest = match serde_json::from_slice(bytes) {
        Ok(request) => request,
        Err(_) => return STATUS_INVALID_JSON,
    };
    let Ok(guard) = handle.state.lock() else {
        return STATUS_INVALID_ARGUMENT;
    };
    let transaction = match guard.plan(request) {
        Ok(transaction) => transaction,
        Err(error) => return error.status(),
    };
    // SAFETY: All ranges were validated as disjoint above.
    unsafe { write_json(&transaction, output, output_capacity, output_length) }
}

/// Atomically applies a patch produced by the state planner.
///
/// # Safety
///
/// The handle must be live and input readable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_apply_patch_json(
    handle: *mut AviQtlTimelineState,
    input: *const u8,
    input_length: usize,
) -> u32 {
    // SAFETY: The caller upholds the handle and input contracts above.
    let (Some(handle), Some(bytes)) = (unsafe { state_from_handle(handle) }, unsafe {
        input_bytes(input, input_length)
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let patch: Patch = match serde_json::from_slice(bytes) {
        Ok(patch) => patch,
        Err(_) => return STATUS_INVALID_JSON,
    };
    let Ok(mut guard) = handle.state.lock() else {
        return STATUS_INVALID_ARGUMENT;
    };
    match guard.apply_patch(&patch) {
        Ok(()) => STATUS_OK,
        Err(error) => error.status(),
    }
}

unsafe fn reserve_ids(
    handle: *mut AviQtlTimelineState,
    kind: EntityKind,
    count: usize,
    output: *mut i32,
    output_length: usize,
) -> u32 {
    if count != output_length || !slice_is_valid(output, output_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The caller upholds the handle contract.
    let Some(handle) = (unsafe { state_from_handle(handle) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Ok(mut guard) = handle.state.lock() else {
        return STATUS_INVALID_ARGUMENT;
    };
    let ids = match guard.reserve_ids(kind, count) {
        Ok(ids) => ids,
        Err(error) => return error.status(),
    };
    if !ids.is_empty() {
        // SAFETY: output was validated for exactly ids.len() writable elements.
        unsafe { std::ptr::copy_nonoverlapping(ids.as_ptr(), output, ids.len()) };
    }
    STATUS_OK
}

/// Reserves monotonically increasing clip IDs from Rust-owned state.
///
/// # Safety
///
/// The handle must be live and output writable for exactly `count` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_reserve_clip_ids(
    handle: *mut AviQtlTimelineState,
    count: usize,
    output: *mut i32,
    output_length: usize,
) -> u32 {
    // SAFETY: The caller upholds the contracts above.
    unsafe { reserve_ids(handle, EntityKind::Clip, count, output, output_length) }
}

/// Reserves monotonically increasing scene IDs from Rust-owned state.
///
/// # Safety
///
/// The handle must be live and output writable for exactly `count` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_state_reserve_scene_ids(
    handle: *mut AviQtlTimelineState,
    count: usize,
    output: *mut i32,
    output_length: usize,
) -> u32 {
    // SAFETY: The caller upholds the contracts above.
    unsafe { reserve_ids(handle, EntityKind::Scene, count, output, output_length) }
}

unsafe fn next_id(handle: *mut AviQtlTimelineState, kind: EntityKind) -> i32 {
    // SAFETY: The caller upholds the handle contract.
    let Some(handle) = (unsafe { state_from_handle(handle) }) else {
        return -1;
    };
    let Ok(guard) = handle.state.lock() else {
        return -1;
    };
    match kind {
        EntityKind::Clip => guard.next_clip_id,
        EntityKind::Scene => guard.next_scene_id,
    }
}

#[unsafe(no_mangle)]
/// Returns the next clip-ID hint without reserving it.
///
/// # Safety
///
/// The handle must be live for the duration of the call.
pub unsafe extern "C" fn aviqtl_timeline_state_next_clip_id(
    handle: *mut AviQtlTimelineState,
) -> i32 {
    // SAFETY: The caller supplies a live handle.
    unsafe { next_id(handle, EntityKind::Clip) }
}

#[unsafe(no_mangle)]
/// Returns the next scene-ID hint without reserving it.
///
/// # Safety
///
/// The handle must be live for the duration of the call.
pub unsafe extern "C" fn aviqtl_timeline_state_next_scene_id(
    handle: *mut AviQtlTimelineState,
) -> i32 {
    // SAFETY: The caller supplies a live handle.
    unsafe { next_id(handle, EntityKind::Scene) }
}

#[unsafe(no_mangle)]
/// Replaces the clip-ID allocation hint. Existing IDs are still skipped during reservation.
///
/// # Safety
///
/// The handle must be live for the duration of the call.
pub unsafe extern "C" fn aviqtl_timeline_state_set_next_clip_hint(
    handle: *mut AviQtlTimelineState,
    next_hint: i32,
) -> u32 {
    // SAFETY: The caller supplies a live handle.
    let Some(handle) = (unsafe { state_from_handle(handle) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Ok(mut guard) = handle.state.lock() else {
        return STATUS_INVALID_ARGUMENT;
    };
    guard.next_clip_id = next_hint.max(1);
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document() -> ProjectDocument {
        parse_project_document(
            serde_json::to_string(&json!({
                "version": 3,
                "settings": {"width": 1920, "height": 1080, "fps": 60.0, "sampleRate": 48000},
                "scenes": [{
                    "id": 0, "name": "Root", "width": 1920, "height": 1080, "fps": 60.0,
                    "start": 0, "duration": 300, "nestedDuration": 0, "lockedLayers": [],
                    "hiddenLayers": [], "gridMode": "Auto", "gridBpm": 120.0,
                    "gridOffset": 0.0, "gridInterval": 10, "gridSubdivision": 4,
                    "enableSnap": true, "magneticSnapRange": 10
                }],
                "clips": [{
                    "id": 1, "sceneId": 0, "type": "video", "start": 0, "duration": 20,
                    "layer": 0, "clipByUpperObject": false, "params": {},
                    "effects": [{
                        "id": "transform", "name": "Transform", "enabled": true,
                        "params": {"x": 0.0},
                        "keyframes": {"x": {"start": {"frame": 0, "value": 0.0, "interp": "linear"},
                            "points": [{"frame": 15, "value": 15.0, "interp": "linear"}]}}
                    }],
                    "audioPlugins": [{
                        "id": "gain", "enabled": true, "params": {"0": 0.0},
                        "keyframes": {"0": {"start": {"frame": 0, "value": 0.0, "interp": "linear"},
                            "points": [{"frame": 12, "value": 1.0, "interp": "linear"}]}}
                    }]
                }]
            }))
            .expect("test document serializes")
            .as_bytes(),
        )
        .expect("test document parses")
    }

    #[test]
    fn forward_and_inverse_restore_full_clip_payload() {
        let mut state = TimelineState::new(document(), 2, 1).expect("valid state");
        let before = state.document.clone();
        let mut replacement = before.clips[0].clone();
        replacement.start = 12;
        replacement
            .params
            .insert("path".to_owned(), json!("media.mp4"));
        replacement.effects[0].enabled = false;
        replacement.audio_plugins[0]
            .params
            .insert("0".to_owned(), json!(0.75));
        let transaction = state
            .plan(EditRequest::ReplaceClip {
                clip_id: 1,
                clip: replacement,
            })
            .expect("replacement plans");
        state
            .apply_patch(&transaction.forward)
            .expect("forward applies");
        assert_ne!(state.document, before);
        state
            .apply_patch(&transaction.inverse)
            .expect("inverse applies");
        assert_eq!(state.document, before);
    }

    #[test]
    fn patch_application_is_atomic_on_conflict() {
        let mut state = TimelineState::new(document(), 2, 1).expect("valid state");
        let before = state.document.clone();
        let mut changed = before.clips[0].clone();
        changed.start = 10;
        let patch = Patch {
            operations: vec![
                PatchOperation::ReplaceClip {
                    index: 0,
                    before: before.clips[0].clone(),
                    after: changed.clone(),
                },
                PatchOperation::ReplaceClip {
                    index: 0,
                    before: before.clips[0].clone(),
                    after: changed,
                },
            ],
        };
        assert_eq!(state.apply_patch(&patch), Err(StateError::Conflict));
        assert_eq!(state.document, before);
    }

    #[test]
    fn patch_application_rejects_a_negative_clip_start() {
        let mut state = TimelineState::new(document(), 2, 1).expect("valid state");
        let before = state.document.clone();
        let mut changed = before.clips[0].clone();
        changed.start = -1;
        let patch = Patch {
            operations: vec![PatchOperation::ReplaceClip {
                index: 0,
                before: before.clips[0].clone(),
                after: changed,
            }],
        };
        assert_eq!(state.apply_patch(&patch), Err(StateError::InvalidArgument));
        assert_eq!(state.document, before);
    }

    #[test]
    fn targeted_scene_and_clip_updates_preserve_extensions_and_are_reversible() {
        let mut before = document();
        before.scenes[0]
            .extra
            .insert("sceneExtension".to_owned(), json!({"token": 1}));
        before.clips[0]
            .extra
            .insert("clipExtension".to_owned(), json!(["kept"]));
        let mut state = TimelineState::new(before.clone(), 2, 1).expect("valid state");

        let mut updated_scene = before.scenes[0].clone();
        updated_scene.name = "Edited".to_owned();
        updated_scene
            .extra
            .insert("sceneExtension".to_owned(), json!("caller value"));
        let scene_transaction = state
            .plan(EditRequest::UpdateScene {
                scene_id: 0,
                scene: updated_scene,
            })
            .expect("scene update plans");
        state
            .apply_patch(&scene_transaction.forward)
            .expect("scene update applies");
        assert_eq!(state.document.scenes[0].name, "Edited");
        assert_eq!(
            state.document.scenes[0].extra["sceneExtension"],
            json!({"token": 1})
        );

        let geometry_transaction = state
            .plan(EditRequest::UpdateClipGeometry {
                clip_id: 1,
                layer: 3,
                start: 7,
                duration: 10,
            })
            .expect("geometry update plans");
        state
            .apply_patch(&geometry_transaction.forward)
            .expect("geometry update applies");
        assert_eq!(state.document.clips[0].layer, 3);
        assert_eq!(state.document.clips[0].start, 7);
        assert_eq!(state.document.clips[0].duration, 10);
        assert_eq!(
            state.document.clips[0].extra["clipExtension"],
            json!(["kept"])
        );

        let compositing_transaction = state
            .plan(EditRequest::SetClipByUpperObject {
                clip_id: 1,
                enabled: true,
            })
            .expect("compositing update plans");
        state
            .apply_patch(&compositing_transaction.forward)
            .expect("compositing update applies");
        assert!(state.document.clips[0].clip_by_upper_object);

        state
            .apply_patch(&compositing_transaction.inverse)
            .expect("compositing inverse applies");
        state
            .apply_patch(&geometry_transaction.inverse)
            .expect("geometry inverse applies");
        state
            .apply_patch(&scene_transaction.inverse)
            .expect("scene inverse applies");
        assert_eq!(state.document, before);
    }

    #[test]
    fn targeted_effect_edits_preserve_extensions_and_are_reversible() {
        let mut before = document();
        before.clips[0].effects[0]
            .extra
            .insert("effectExtension".to_owned(), json!("transform-token"));
        let mut blur = before.clips[0].effects[0].clone();
        blur.id = "blur".to_owned();
        blur.name = "Blur".to_owned();
        blur.keyframes = None;
        blur.extra
            .insert("effectExtension".to_owned(), json!("blur-token"));
        let mut color = blur.clone();
        color.id = "color".to_owned();
        color.name = "Color".to_owned();
        color
            .extra
            .insert("effectExtension".to_owned(), json!("color-token"));
        before.clips[0].effects.extend([blur, color]);
        let mut state = TimelineState::new(before.clone(), 2, 1).expect("valid state");

        let enabled_transaction = state
            .plan(EditRequest::SetEffectEnabled {
                clip_id: 1,
                effect_index: 1,
                enabled: false,
            })
            .expect("enabled update plans");
        state
            .apply_patch(&enabled_transaction.forward)
            .expect("enabled update applies");
        assert!(!state.document.clips[0].effects[1].enabled);
        assert_eq!(
            state.document.clips[0].effects[1].extra["effectExtension"],
            json!("blur-token")
        );
        state
            .apply_patch(&enabled_transaction.inverse)
            .expect("enabled inverse applies");
        assert_eq!(state.document, before);

        let reorder_transaction = state
            .plan(EditRequest::ReorderEffects {
                clip_id: 1,
                permutation: vec![0, 2, 1],
            })
            .expect("reorder plans");
        state
            .apply_patch(&reorder_transaction.forward)
            .expect("reorder applies");
        assert_eq!(state.document.clips[0].effects[1].id, "color");
        assert_eq!(
            state.document.clips[0].effects[1].extra["effectExtension"],
            json!("color-token")
        );
        state
            .apply_patch(&reorder_transaction.inverse)
            .expect("reorder inverse applies");
        assert_eq!(state.document, before);

        let remove_transaction = state
            .plan(EditRequest::RemoveEffects {
                clip_id: 1,
                effect_indices: vec![2, 1],
            })
            .expect("removal plans");
        state
            .apply_patch(&remove_transaction.forward)
            .expect("removal applies");
        assert_eq!(state.document.clips[0].effects.len(), 1);
        state
            .apply_patch(&remove_transaction.inverse)
            .expect("removal inverse applies");
        assert_eq!(state.document, before);

        let mut inserted = before.clips[0].effects[1].clone();
        inserted.id = "mask".to_owned();
        inserted.name = "Mask".to_owned();
        inserted
            .extra
            .insert("effectExtension".to_owned(), json!("mask-token"));
        let insert_transaction = state
            .plan(EditRequest::InsertEffects {
                clip_id: 1,
                insertions: vec![EffectInsertion {
                    index: 2,
                    effect: inserted,
                }],
            })
            .expect("insertion plans");
        state
            .apply_patch(&insert_transaction.forward)
            .expect("insertion applies");
        assert_eq!(state.document.clips[0].effects[2].id, "mask");
        assert_eq!(
            state.document.clips[0].effects[2].extra["effectExtension"],
            json!("mask-token")
        );
        state
            .apply_patch(&insert_transaction.inverse)
            .expect("insertion inverse applies");
        assert_eq!(state.document, before);

        assert_eq!(
            state.plan(EditRequest::RemoveEffects {
                clip_id: 1,
                effect_indices: vec![0],
            }),
            Err(StateError::InvalidArgument)
        );
        assert_eq!(
            state.plan(EditRequest::ReorderEffects {
                clip_id: 1,
                permutation: vec![1, 0, 2],
            }),
            Err(StateError::InvalidArgument)
        );
    }

    #[test]
    fn batch_geometry_updates_preserve_extensions_and_are_reversible() {
        let mut before = document();
        before.clips[0]
            .extra
            .insert("clipExtension".to_owned(), json!("first"));
        let mut second = before.clips[0].clone();
        second.id = 2;
        second.start = 40;
        second
            .extra
            .insert("clipExtension".to_owned(), json!("second"));
        before.clips.push(second);
        let mut state = TimelineState::new(before.clone(), 3, 1).expect("valid state");

        let transaction = state
            .plan(EditRequest::BatchUpdateClipGeometry {
                updates: vec![
                    ClipGeometryUpdate {
                        clip_id: 1,
                        layer: 2,
                        start: 5,
                        duration: 20,
                    },
                    ClipGeometryUpdate {
                        clip_id: 2,
                        layer: 3,
                        start: 50,
                        duration: 25,
                    },
                ],
            })
            .expect("batch update plans");
        state
            .apply_patch(&transaction.forward)
            .expect("batch update applies");
        assert_eq!(state.document.clips[0].layer, 2);
        assert_eq!(state.document.clips[0].start, 5);
        assert_eq!(
            state.document.clips[0].extra["clipExtension"],
            json!("first")
        );
        assert_eq!(state.document.clips[1].layer, 3);
        assert_eq!(state.document.clips[1].start, 50);
        assert_eq!(
            state.document.clips[1].extra["clipExtension"],
            json!("second")
        );

        state
            .apply_patch(&transaction.inverse)
            .expect("batch inverse applies");
        assert_eq!(state.document, before);
    }

    #[test]
    fn split_is_reversible_and_splits_effect_and_plugin_tracks() {
        let mut state = TimelineState::new(document(), 2, 1).expect("valid state");
        let before = state.document.clone();
        let transaction = state
            .plan(EditRequest::SplitClip {
                clip_id: 1,
                frame: 10,
                new_clip_id: 2,
            })
            .expect("split plans");
        state
            .apply_patch(&transaction.forward)
            .expect("split applies");
        assert_eq!(state.document.clips.len(), 2);
        assert_eq!(state.document.clips[0].duration, 10);
        assert_eq!(state.document.clips[1].start, 10);
        let second_effect_tracks = state.document.clips[1].effects[0]
            .keyframes
            .as_ref()
            .expect("effect tracks retained");
        let second_plugin_tracks = state.document.clips[1].audio_plugins[0]
            .keyframes
            .as_ref()
            .expect("plugin tracks retained");
        assert!(second_effect_tracks.contains_key("x"));
        assert!(second_plugin_tracks.contains_key("0"));
        state
            .apply_patch(&transaction.inverse)
            .expect("split inverse applies");
        assert_eq!(state.document, before);
    }

    #[test]
    fn removing_scene_with_clips_is_one_reversible_transaction() {
        let mut state = TimelineState::new(document(), 2, 1).expect("valid state");
        let before = state.document.clone();
        let transaction = state
            .plan(EditRequest::RemoveScene { scene_id: 0 })
            .expect("removal plans");
        state
            .apply_patch(&transaction.forward)
            .expect("forward removes clips before scene");
        assert!(state.document.scenes.is_empty());
        assert!(state.document.clips.is_empty());
        state
            .apply_patch(&transaction.inverse)
            .expect("inverse restores scene before clips");
        assert_eq!(state.document, before);
    }

    #[test]
    fn id_reservation_is_monotonic_and_skips_existing_ids() {
        let mut state = TimelineState::new(document(), 1, 1).expect("valid state");
        assert_eq!(state.reserve_ids(EntityKind::Clip, 3).unwrap(), [2, 3, 4]);
        assert_eq!(state.reserve_ids(EntityKind::Clip, 2).unwrap(), [5, 6]);
        assert_eq!(state.reserve_ids(EntityKind::Scene, 2).unwrap(), [1, 2]);
    }
}
