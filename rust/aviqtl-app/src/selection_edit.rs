//! Multi-object property editing.
//!
//! When several objects are selected, a property control has to answer one question for many
//! clips: what does this parameter currently read? The three answers are uniform (every selected
//! object holds the same value), mixed (the objects disagree), and absent (the object has no such
//! parameter at all). Those states are what a sidebar shows, and they are also what decides the
//! result of an edit: editing a mixed value writes to every object that *has* it, and an object
//! that lacks it is skipped rather than given a parameter it never had.
//!
//! Two rules drive everything here, and both come from how AviUtl actually behaves rather than
//! from what would be tidier to implement:
//!
//! 1. **Effects are matched by catalog id, not by index.** Two clips usually hold the same effect
//!    at different sidebar positions — a rectangle clip carries `transform` first while a text
//!    clip carries `text` first. Matching by index would edit a different effect on one of them,
//!    which is worse than not editing at all.
//! 2. **One edit is one undo step.** The whole selection is planned as a single batch, so Ctrl+Z
//!    restores every object the edit touched together, exactly as the single-object path does.
//!
//! This module is pure: it reads a project and reports what would change. It performs no I/O,
//! holds no state, and never mutates the document. [`crate::workspace::WorkspaceModel`] applies
//! the plan it returns through the one mutation funnel, so undo, redo, dirty tracking and the
//! continuous-edit session all come for free.

use crate::object_settings::{ObjectControlKind, replace_value_payload, value_payload};
use aviqtl_rust_core::api::{
    AudioPluginDocument, ClipDocument, EffectDocument, ProjectDocument, TimelineCommand,
    inspect_keyframe_track,
};
use serde_json::Value;
use std::collections::BTreeSet;

/// How a single parameter reads across a multi-object selection.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionValue {
    /// Every selected object holds exactly this value at the playhead.
    Uniform(Value),
    /// The objects that have the parameter disagree. The payload is the first value in document
    /// order, which is only a display hint — an edit replaces it everywhere.
    Mixed(Value),
    /// No selected object has this parameter, so there is nothing to display or edit.
    Absent,
}

impl SelectionValue {
    /// The value a control should display. A mixed selection shows the first value, because every
    /// mainstream editor shows *something* and marks it as mixed rather than showing nothing.
    pub fn display_value(&self) -> Option<&Value> {
        match self {
            Self::Uniform(value) | Self::Mixed(value) => Some(value),
            Self::Absent => None,
        }
    }

    /// Whether the objects disagree. A sidebar uses this to show its mixed-state affordance.
    pub fn is_mixed(&self) -> bool {
        matches!(self, Self::Mixed(_))
    }

    /// Whether the value should be drawn in the placeholder style, which every editor does for a
    /// value that is not really the selection's value.
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

/// The aggregate state of one property control across the selection.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionProperty {
    /// The catalog id of the effect this property belongs to, or the audio plugin id.
    pub effect_id: String,
    /// The parameter name, or `None` for a control that is not backed by a parameter (a header or
    /// a label).
    pub param: Option<String>,
    /// What the parameter reads across the selection.
    pub value: SelectionValue,
    /// How many selected objects have this parameter, including objects on locked layers. This is
    /// the denominator the sidebar uses to report an edit, and the count `present_objects()`
    /// returns for a uniform or mixed value.
    pub editable_objects: usize,
}

impl SelectionProperty {
    /// The number of selected objects that hold this parameter. A smaller number than the
    /// selection size is the normal case for mixed selections of dissimilar objects.
    pub fn present_objects(&self) -> usize {
        match &self.value {
            SelectionValue::Absent => 0,
            SelectionValue::Uniform(_) => self.editable_objects,
            SelectionValue::Mixed(_) => self.editable_objects,
        }
    }
}

/// The result of resolving one property across a multi-object selection.
///
/// `TimelineCommand` is neither `Clone` nor `PartialEq`, so this derives only `Debug`: the
/// commands are handed to the mutation funnel rather than compared.
#[derive(Debug)]
pub struct SelectionPlan {
    /// One command per object that will actually change. Empty when the edit is a no-op, which is
    /// what keeps a no-op from pushing an undo entry.
    pub commands: Vec<TimelineCommand>,
    /// How many selected objects the selection contained.
    pub selected_objects: usize,
    /// How many of them the edit will touch.
    pub edited_objects: usize,
    /// How many were skipped because they are on a locked layer.
    pub locked_objects: usize,
    /// How many were skipped because they do not carry the effect or the parameter.
    pub absent_objects: usize,
}

impl SelectionPlan {
    /// Whether the plan would change anything. An empty command list is a no-op, so a caller can
    /// skip the whole mutation funnel and its undo bookkeeping.
    pub fn is_no_op(&self) -> bool {
        self.commands.is_empty()
    }
}

/// What a multi-object edit actually did, reported back to the caller once the commands have been
/// consumed by the mutation funnel.
///
/// A sidebar uses this for its "edited 3 of 5 objects" line. It is separate from [`SelectionPlan`]
/// because the plan owns the commands and they are moved into the funnel, so it cannot also be
/// the caller's receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionApplied {
    /// How many objects were selected.
    pub selected_objects: usize,
    /// How many were actually changed.
    pub edited_objects: usize,
    /// How many were skipped because their layer is locked.
    pub locked_objects: usize,
    /// How many were skipped because they do not carry the effect or the parameter.
    pub absent_objects: usize,
}

impl SelectionApplied {
    /// How many selected objects the edit left alone.
    pub fn skipped_objects(&self) -> usize {
        self.locked_objects + self.absent_objects
    }
}

/// Resolves a single parameter across the selection, without planning an edit.
///
/// This is the read half of the feature: it is what a sidebar calls to decide between drawing a
/// plain value, a mixed placeholder, or nothing at all. It deliberately does not consult the
/// catalog, because a parameter that is present on a clip is editable whether or not the catalog
/// describes it — an unknown effect's parameter is still the user's data, and refusing to show it
/// would silently hide content this project stores.
pub fn selection_value(
    clips: &[&ClipDocument],
    effect_id: &str,
    param: &str,
    playhead: i32,
) -> SelectionValue {
    let mut found: Option<Value> = None;
    let mut mixed = false;
    for clip in clips {
        // The effect is found by catalog id, never by position, so a clip that orders its effects
        // differently still resolves to the same effect.
        let Some(effect) = clip.effects.iter().find(|effect| effect.id == effect_id) else {
            continue;
        };
        let Some(original) = effect.params.get(param) else {
            continue;
        };
        let current = value_at_playhead(effect, param, original, clip, playhead);
        match &found {
            None => found = Some(current),
            Some(previous) if value_payload(previous) == value_payload(&current) => {}
            Some(_) => mixed = true,
        }
    }
    match found {
        None => SelectionValue::Absent,
        Some(value) if mixed => SelectionValue::Mixed(value),
        Some(value) => SelectionValue::Uniform(value),
    }
}

/// Plans a multi-object parameter edit.
///
/// The rules, in order:
///
/// * Objects on a locked layer are **skipped, not an error**. A locked clip keeps its value and is
///   excluded from the edit, matching the existing move and resize behaviour. This is why the
///   plan reports `locked_objects` instead of failing.
/// * Objects without the effect, or without the parameter on that effect, are **skipped** and
///   counted in `absent_objects`. A selection of a rectangle and a text clip editing `blur.size`
///   edits the rectangle and leaves the text clip untouched.
/// * A parameter that is keyframed on one object is edited **at the playhead** on that object, and
///   as a plain parameter on the objects that have no track for it. This mirrors the existing
///   single-object `set_effect_param_for_clip` rule exactly, so the same gesture means the same
///   thing whether one object or ten are selected.
/// * Objects are visited in **document order**, so a Ctrl-click in a different order produces an
///   identical edit.
pub fn plan_selection_param_edit(
    document: &ProjectDocument,
    scene_id: i32,
    selected_ids: &[i32],
    effect_id: &str,
    param: &str,
    value: Value,
    playhead: i32,
) -> SelectionPlan {
    let Some(scene) = document.scenes.iter().find(|scene| scene.id == scene_id) else {
        return empty_plan();
    };
    // The applied set is the *selection* intersected with the scene, visited in document order, so
    // a Ctrl-click in a different order produces an identical edit.
    let clips: Vec<&ClipDocument> = document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene_id && selected_ids.contains(&clip.id))
        .collect();
    let mut plan = SelectionPlan {
        commands: Vec::new(),
        selected_objects: clips.len(),
        locked_objects: 0,
        absent_objects: 0,
        edited_objects: 0,
    };

    for clip in clips {
        // The effect is found by catalog id, never by index, so the command targets the effect's
        // real position on this object even when the object orders its effects differently.
        let Some(effect_index) = clip
            .effects
            .iter()
            .position(|effect| effect.id == effect_id)
        else {
            plan.absent_objects += 1;
            continue;
        };
        let Some(original) = clip.effects[effect_index].params.get(param) else {
            plan.absent_objects += 1;
            continue;
        };
        let effect = &clip.effects[effect_index];
        // The payload carries the value's type metadata, which each object's own type may
        // differ in, so the incoming payload is re-wrapped per object rather than copied.
        let value = replace_value_payload(original, value.clone());
        if scene.locked_layers.contains(&clip.layer) {
            plan.locked_objects += 1;
            continue;
        }
        // An edit that would write the value the object already holds is not a change, so it is
        // left out. This is what makes the plan a no-op when a mixed selection is resolved back to
        // the value most objects already have.
        let current = value_at_playhead(effect, param, original, clip, playhead);
        let track = effect
            .keyframes
            .as_ref()
            .and_then(|tracks| tracks.get(param));
        let command = match track {
            Some(track) => {
                let frame = playhead
                    .saturating_sub(clip.start)
                    .clamp(0, clip.duration.max(0));
                let points = inspect_keyframe_track(Some(track), original, clip.duration);
                // A keyframe that already exists at this frame and already holds this value is not
                // a change. Without the frame check the no-op rule would be wrong, because the
                // evaluated value alone can match while the keyframe itself sits elsewhere.
                if points.iter().any(|point| {
                    point.frame == frame && value_payload(&point.value) == value_payload(&value)
                }) {
                    continue;
                }
                let options = crate::workspace::keyframe_options_for_points(&points, frame);
                TimelineCommand::SetEffectKeyframe {
                    clip_id: clip.id,
                    effect_index,
                    param_name: param.to_owned(),
                    frame,
                    value,
                    options,
                }
            }
            None => {
                if value_payload(&current) == value_payload(&value) {
                    continue;
                }
                TimelineCommand::SetEffectParameter {
                    clip_id: clip.id,
                    effect_index,
                    param_name: param.to_owned(),
                    value,
                    media_duration_seconds: None,
                }
            }
        };
        plan.edited_objects += 1;
        plan.commands.push(command);
    }
    plan
}

/// Resolves an audio plugin parameter across the selection.
///
/// Audio plugins are tracked by numeric index string rather than by catalog id, because a clip
/// has no notion of an audio-plugin identity beyond its position in the list. That makes the
/// index the identity here, and it is why audio plugins get their own entry point rather than
/// sharing the effect one.
pub fn plan_selection_audio_param_edit(
    document: &ProjectDocument,
    scene_id: i32,
    selected_ids: &[i32],
    plugin_index: usize,
    param: &str,
    value: Value,
) -> SelectionPlan {
    let Some(scene) = document.scenes.iter().find(|scene| scene.id == scene_id) else {
        return empty_plan();
    };
    let clips: Vec<&ClipDocument> = document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene_id && selected_ids.contains(&clip.id))
        .collect();
    let mut plan = SelectionPlan {
        commands: Vec::new(),
        selected_objects: clips.len(),
        locked_objects: 0,
        absent_objects: 0,
        edited_objects: 0,
    };

    for clip in clips {
        let Some(plugin) = clip.audio_plugins.get(plugin_index) else {
            plan.absent_objects += 1;
            continue;
        };
        let Some(original) = plugin.params.get(param) else {
            plan.absent_objects += 1;
            continue;
        };
        let value = replace_value_payload(original, value.clone());
        if scene.locked_layers.contains(&clip.layer) {
            plan.locked_objects += 1;
            continue;
        }
        if value_payload(original) == value_payload(&value) {
            continue;
        }
        plan.edited_objects += 1;
        plan.commands
            .push(TimelineCommand::SetAudioPluginParameter {
                clip_id: clip.id,
                plugin_index,
                param_name: param.to_owned(),
                value,
            });
    }
    plan
}

/// Resolves every property of every effect across the selection, which is what a sidebar needs to
/// build its whole control tree for a multi-object selection.
///
/// A property appears when at least one selected object has it. Controls that are not backed by a
/// parameter are not included here; the sidebar already builds those from the catalog.
pub fn selection_properties(clips: &[&ClipDocument], playhead: i32) -> Vec<SelectionProperty> {
    let mut order: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut properties = Vec::new();
    for clip in clips {
        for effect in &clip.effects {
            if !seen.insert(effect.id.clone()) {
                continue;
            }
            order.push(effect.id.clone());
        }
    }
    for effect_id in order {
        let mut names: BTreeSet<String> = BTreeSet::new();
        for clip in clips {
            if let Some(effect) = clip.effects.iter().find(|effect| effect.id == effect_id) {
                names.extend(effect.params.keys().cloned());
            }
        }
        for param in names {
            let value = selection_value(clips, &effect_id, &param, playhead);
            let editable_objects = clips
                .iter()
                .filter(|clip| {
                    clip.effects
                        .iter()
                        .find(|effect| effect.id == effect_id)
                        .is_some_and(|effect| effect.params.contains_key(&param))
                })
                .count();
            properties.push(SelectionProperty {
                effect_id: effect_id.clone(),
                param: Some(param),
                value,
                editable_objects,
            });
        }
    }
    properties
}

/// Whether every control kind can carry a multi-object edit.
///
/// Only the kinds that hold a single value do. A header is not a value, and an unsupported control
/// has no parameter to write, so both are excluded rather than silently accepted and dropped.
pub fn control_kind_is_editable(kind: ObjectControlKind) -> bool {
    !matches!(
        kind,
        ObjectControlKind::Header | ObjectControlKind::Unsupported
    )
}

/// The value a parameter reads at the playhead, following a keyframe track when one exists.
///
/// This is the same evaluation the single-object sidebar uses, so a multi-object control and a
/// single-object control never disagree about what a parameter currently reads.
fn value_at_playhead(
    effect: &EffectDocument,
    param: &str,
    original: &Value,
    clip: &ClipDocument,
    playhead: i32,
) -> Value {
    let Some(track) = effect
        .keyframes
        .as_ref()
        .and_then(|tracks| tracks.get(param))
    else {
        return original.clone();
    };
    let frame = playhead
        .saturating_sub(clip.start)
        .clamp(0, clip.duration.max(0));
    evaluated_keyframe_value(track, original, clip, frame)
}

fn evaluated_keyframe_value(
    track: &Value,
    original: &Value,
    clip: &ClipDocument,
    frame: i32,
) -> Value {
    aviqtl_rust_core::api::evaluate_keyframe_track(Some(track), original, clip.duration, frame)
}

/// An empty plan for a scene that does not exist, so every caller gets a plan rather than an
/// `Option` it has to unwrap at each step.
fn empty_plan() -> SelectionPlan {
    SelectionPlan {
        commands: Vec::new(),
        selected_objects: 0,
        locked_objects: 0,
        absent_objects: 0,
        edited_objects: 0,
    }
}

/// The parameter name a control carries, if any. A header control has none.
pub fn control_param(control: &crate::object_settings::ObjectControl) -> Option<&str> {
    control.param.as_deref()
}

/// A convenience for reading the audio plugin documents of a clip, kept here so the two
/// multi-object paths agree on what an audio clip carries.
pub fn audio_plugins(clip: &ClipDocument) -> &[AudioPluginDocument] {
    &clip.audio_plugins
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::TimelineState;
    use serde_json::json;

    /// Builds a clip document through the real parse path, so the defaults `TimelineState`
    /// applies (such as an effect's `enabled` flag) are the same ones a loaded project gets.
    fn clip_json(id: i32, scene: i32, layer: i32, effects: Value) -> Value {
        json!({
            "id": id,
            "sceneId": scene,
            "type": "rect",
            "start": 0,
            "duration": 100,
            "layer": layer,
            "effects": effects
        })
    }

    fn project(clips: Vec<Value>) -> ProjectDocument {
        project_with_scene(clips, json!({"id": 1, "name": "Root", "duration": 300}))
    }

    fn project_with_scene(clips: Vec<Value>, scene: Value) -> ProjectDocument {
        let json = json!({
            "version": 3,
            "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
            "scenes": [scene],
            "clips": clips
        });
        TimelineState::from_json(json.to_string().as_bytes())
            .expect("document parses")
            .snapshot()
    }

    fn clips_of(document: &ProjectDocument) -> Vec<&ClipDocument> {
        document.clips.iter().collect()
    }

    fn rectangle_effects(count: i64) -> Value {
        json!([{
            "id": "rect",
            "name": "Rectangle",
            "params": {"count": {"$aviqtlType": "int", "value": count, "extension": 7}}
        }])
    }

    #[test]
    fn a_uniform_selection_reads_one_value_and_a_disagreeing_one_reads_mixed() {
        let project_document = project(vec![
            clip_json(1, 1, 0, rectangle_effects(5)),
            clip_json(2, 1, 0, rectangle_effects(5)),
        ]);
        let refs = clips_of(&project_document);
        assert_eq!(
            selection_value(&refs, "rect", "count", 0),
            SelectionValue::Uniform(json!({"$aviqtlType": "int", "value": 5, "extension": 7}))
        );

        let disagreeing = project(vec![
            clip_json(1, 1, 0, rectangle_effects(5)),
            clip_json(2, 1, 0, rectangle_effects(9)),
        ]);
        let refs = clips_of(&disagreeing);
        let mixed = selection_value(&refs, "rect", "count", 0);
        assert!(mixed.is_mixed(), "the objects disagree");
        // The payload is the first value in document order, which is only a display hint.
        assert_eq!(
            mixed.display_value(),
            Some(&json!({"$aviqtlType": "int", "value": 5, "extension": 7}))
        );

        let absent = selection_value(&refs, "blur", "size", 0);
        assert!(absent.is_absent());
        assert_eq!(absent.display_value(), None);
    }

    #[test]
    fn a_keyframed_parameter_reads_at_the_playhead_for_every_object() {
        let keyframed = project(vec![clip_json(
            1,
            1,
            0,
            json!([{
                "id": "rect",
                "params": {"size": 0},
                "keyframes": {"size": [{"frame": 0, "value": 0}, {"frame": 40, "value": 100}]}
            }]),
        )]);
        let refs = clips_of(&keyframed);
        // At frame 20 the track interpolates to the midpoint, so the control shows 50 rather than
        // the stored base value of 0. The interpolation is arithmetic on the track's numbers, so
        // the result is a float; the payload comparison is what decides uniformity, not the
        // JSON number spelling.
        let at_twenty = selection_value(&refs, "rect", "size", 20);
        assert!(!at_twenty.is_mixed());
        assert_eq!(
            at_twenty.display_value().and_then(Value::as_f64),
            Some(50.0),
            "the playhead reads the interpolated value"
        );
        let at_sixty = selection_value(&refs, "rect", "size", 60);
        assert_eq!(
            at_sixty.display_value().and_then(Value::as_f64),
            Some(100.0),
            "past the last keyframe the track holds its final value"
        );
    }

    #[test]
    fn editing_the_selection_writes_to_every_object_in_one_plan() {
        let project_document = project(vec![
            clip_json(1, 1, 0, rectangle_effects(5)),
            clip_json(2, 1, 0, rectangle_effects(9)),
        ]);
        let plan =
            plan_selection_param_edit(&project_document, 1, &[1, 2], "rect", "count", json!(7), 0);
        assert_eq!(plan.selected_objects, 2);
        assert_eq!(
            plan.edited_objects, 2,
            "a mixed value is replaced everywhere"
        );
        assert_eq!(plan.commands.len(), 2);
        // Document order, not selection order.
        let ids: Vec<i32> = plan
            .commands
            .iter()
            .map(|command| match command {
                TimelineCommand::SetEffectParameter { clip_id, .. } => *clip_id,
                other => panic!("unexpected command {other:?}"),
            })
            .collect();
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn an_object_without_the_parameter_is_skipped_rather_than_given_one() {
        let project_document = project(vec![
            clip_json(1, 1, 0, rectangle_effects(5)),
            // The second object carries `blur`, not `rect`, so `rect.count` does not exist on it.
            clip_json(2, 1, 0, json!([{"id": "blur", "params": {"size": 3}}])),
        ]);
        let plan =
            plan_selection_param_edit(&project_document, 1, &[1, 2], "rect", "count", json!(7), 0);
        assert_eq!(
            plan.edited_objects, 1,
            "only the object that has the parameter"
        );
        assert_eq!(plan.absent_objects, 1);
        assert_eq!(plan.commands.len(), 1);
    }

    #[test]
    fn effects_are_matched_by_catalog_id_so_a_different_order_still_resolves() {
        // The second object holds the rectangle effect at a different sidebar position, which is
        // what a text clip carrying `text` first looks like.
        let project_document = project(vec![
            clip_json(1, 1, 0, rectangle_effects(5)),
            clip_json(
                2,
                1,
                0,
                json!([
                    {"id": "text", "params": {"content": "hello"}},
                    {"id": "rect", "params": {"count": 9}}
                ]),
            ),
        ]);
        let plan =
            plan_selection_param_edit(&project_document, 1, &[1, 2], "rect", "count", json!(7), 0);
        assert_eq!(
            plan.edited_objects, 2,
            "both objects resolve the effect by id"
        );
        // The command targets the rectangle's real index on each object, not index 0.
        let indices: Vec<usize> = plan
            .commands
            .iter()
            .map(|command| match command {
                TimelineCommand::SetEffectParameter { effect_index, .. } => *effect_index,
                other => panic!("unexpected command {other:?}"),
            })
            .collect();
        assert_eq!(indices, [0, 1]);
    }

    #[test]
    fn a_locked_object_is_skipped_and_counted_rather_than_failing_the_edit() {
        let project_document = project_with_scene(
            vec![
                clip_json(1, 1, 0, rectangle_effects(5)),
                clip_json(2, 1, 1, rectangle_effects(5)),
            ],
            json!({"id": 1, "name": "Root", "duration": 300, "lockedLayers": [1]}),
        );
        let plan =
            plan_selection_param_edit(&project_document, 1, &[1, 2], "rect", "count", json!(7), 0);
        assert_eq!(plan.selected_objects, 2);
        assert_eq!(plan.edited_objects, 1, "the locked object keeps its value");
        assert_eq!(plan.locked_objects, 1);
        assert_eq!(plan.commands.len(), 1);
    }

    #[test]
    fn a_keyframed_object_is_edited_at_the_playhead_and_a_plain_one_is_not() {
        let project_document = project(vec![
            clip_json(
                1,
                1,
                0,
                json!([{
                    "id": "rect",
                    "params": {"size": 0},
                    "keyframes": {"size": [{"frame": 0, "value": 0}, {"frame": 80, "value": 80}]}
                }]),
            ),
            clip_json(2, 1, 0, json!([{"id": "rect", "params": {"size": 3}}])),
        ]);
        let plan =
            plan_selection_param_edit(&project_document, 1, &[1, 2], "rect", "size", json!(40), 40);
        assert_eq!(plan.edited_objects, 2);
        let keyed = plan
            .commands
            .iter()
            .find(|command| matches!(command, TimelineCommand::SetEffectKeyframe { .. }))
            .expect("the keyframed object is edited on its track");
        match keyed {
            TimelineCommand::SetEffectKeyframe { clip_id, frame, .. } => {
                assert_eq!(*clip_id, 1);
                assert_eq!(*frame, 40, "the track is edited at the playhead");
            }
            other => panic!("unexpected command {other:?}"),
        }
        assert!(
            plan.commands.iter().any(|command| matches!(
                command,
                TimelineCommand::SetEffectParameter { clip_id: 2, .. }
            )),
            "the object without a track is edited as a plain parameter"
        );
    }

    #[test]
    fn writing_a_value_an_object_already_holds_plans_nothing() {
        let project_document = project(vec![clip_json(1, 1, 0, rectangle_effects(5))]);
        let plan =
            plan_selection_param_edit(&project_document, 1, &[1, 2], "rect", "count", json!(5), 0);
        assert!(plan.is_no_op(), "an unchanged value is not an edit");
        assert_eq!(plan.selected_objects, 1);
        assert_eq!(plan.edited_objects, 0);
    }

    #[test]
    fn a_plan_for_a_missing_scene_is_empty_rather_than_an_error() {
        let project_document = project(vec![clip_json(1, 1, 0, rectangle_effects(5))]);
        let plan =
            plan_selection_param_edit(&project_document, 99, &[1], "rect", "count", json!(7), 0);
        assert!(plan.is_no_op());
        assert_eq!(plan.selected_objects, 0);
    }

    fn audio_clip(id: i32, layer: i32, gain: Option<i64>) -> Value {
        let mut clip = clip_json(id, 1, layer, json!([]));
        clip["type"] = json!("audio");
        if let Some(gain) = gain {
            clip["audioPlugins"] = json!([{"id": "gain", "params": {"gain": gain}}]);
        }
        clip
    }

    #[test]
    fn an_audio_plugin_parameter_is_edited_by_its_index_across_the_selection() {
        let project_document = project(vec![audio_clip(1, 0, Some(0)), audio_clip(2, 1, Some(10))]);
        let plan =
            plan_selection_audio_param_edit(&project_document, 1, &[1, 2], 0, "gain", json!(6));
        assert_eq!(plan.edited_objects, 2);

        // An object with no plugin at that index is skipped, not given one.
        let mixed = project(vec![audio_clip(1, 0, Some(0)), audio_clip(2, 1, None)]);
        let plan = plan_selection_audio_param_edit(&mixed, 1, &[1, 2], 0, "gain", json!(6));
        assert_eq!(plan.edited_objects, 1);
        assert_eq!(plan.absent_objects, 1);
    }

    #[test]
    fn selection_properties_list_a_property_once_with_its_own_state() {
        let project_document = project(vec![
            clip_json(1, 1, 0, rectangle_effects(5)),
            clip_json(
                2,
                1,
                0,
                json!([{
                    "id": "rect",
                    "params": {"count": 9, "opacity": 255}
                }]),
            ),
        ]);
        let refs = clips_of(&project_document);
        let properties = selection_properties(&refs, 0);
        let count = properties
            .iter()
            .find(|property| property.param.as_deref() == Some("count"))
            .expect("count is listed once");
        assert!(count.value.is_mixed());
        assert_eq!(count.editable_objects, 2);
        // `opacity` exists on only one object, so it reads as uniform but is present on one object.
        let opacity = properties
            .iter()
            .find(|property| property.param.as_deref() == Some("opacity"))
            .expect("opacity is listed");
        assert_eq!(opacity.editable_objects, 1);
        assert_eq!(opacity.present_objects(), 1);
    }

    #[test]
    fn only_value_backed_controls_are_editable() {
        assert!(control_kind_is_editable(ObjectControlKind::Number));
        assert!(control_kind_is_editable(ObjectControlKind::Choice));
        assert!(!control_kind_is_editable(ObjectControlKind::Header));
        assert!(!control_kind_is_editable(ObjectControlKind::Unsupported));
    }
}
