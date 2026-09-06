use crate::effect_catalog::EffectCatalog;
use aviqtl_rust_core::api::{
    ClipDocument, EffectDocument, EffectMetadata, ProjectDocument, evaluate_keyframe_track,
    inspect_keyframe_track,
};
pub use aviqtl_rust_core::api::{KeyframePoint, keyframe_interpolation_names};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectControlKind {
    Number,
    Integer,
    Boolean,
    Text,
    Path,
    Color,
    Font,
    Choice,
    Header,
    Unsupported,
}

impl ObjectControlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Text => "text",
            Self::Path => "path",
            Self::Color => "color",
            Self::Font => "font",
            Self::Choice => "choice",
            Self::Header => "header",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectControlOption {
    pub value: Value,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectControl {
    pub kind: ObjectControlKind,
    pub source_kind: String,
    pub param: Option<String>,
    pub label: String,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub step: Option<f64>,
    pub decimals: Option<usize>,
    pub unit: String,
    pub filter: String,
    pub disabled: bool,
    pub keyframed: bool,
    pub value: Value,
    pub relative_frame: i32,
    pub clip_duration: i32,
    pub interval_start: i32,
    pub interval_end: i32,
    pub start_value: Value,
    pub end_value: Value,
    pub start_interpolation: String,
    pub keyframes: Vec<KeyframePoint>,
    pub options: Vec<ObjectControlOption>,
}

impl ObjectControl {
    pub fn display_value(&self) -> String {
        self.display_value_at(&self.value)
    }

    pub fn display_value_at(&self, value: &Value) -> String {
        let value = value_payload(value);
        match value {
            Value::String(value) => value.clone(),
            Value::Number(number) => {
                if let Some(decimals) = self.decimals {
                    number
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .map_or_else(|| number.to_string(), |value| format!("{value:.decimals$}"))
                } else {
                    number.to_string()
                }
            }
            Value::Bool(value) => value.to_string(),
            Value::Null => String::new(),
            value => value.to_string(),
        }
    }

    pub fn number_value(&self) -> f64 {
        self.number_value_at(&self.value)
    }

    pub fn number_value_at(&self, value: &Value) -> f64 {
        value_payload(value)
            .as_f64()
            .filter(|value| value.is_finite())
            .unwrap_or_default()
    }

    pub fn bool_value(&self) -> bool {
        self.bool_value_at(&self.value)
    }

    pub fn bool_value_at(&self, value: &Value) -> bool {
        value_payload(value).as_bool().unwrap_or(false)
    }

    pub fn selected_option(&self) -> Option<usize> {
        self.selected_option_at(&self.value)
    }

    pub fn selected_option_at(&self, value: &Value) -> Option<usize> {
        let current = value_payload(value);
        self.options
            .iter()
            .position(|option| value_payload(&option.value) == current)
    }

    pub fn parse_text(&self, input: &str) -> Result<Value, String> {
        match self.kind {
            ObjectControlKind::Integer => {
                let value = input
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| format!("{}には整数を入力してください", self.label))?;
                let minimum = self.minimum.unwrap_or(i64::MIN as f64).ceil() as i64;
                let maximum = self.maximum.unwrap_or(i64::MAX as f64).floor() as i64;
                Ok(Value::from(if minimum <= maximum {
                    value.clamp(minimum, maximum)
                } else {
                    value
                }))
            }
            ObjectControlKind::Number => {
                let value = input
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| format!("{}には数値を入力してください", self.label))?;
                let minimum = self.minimum.unwrap_or(f64::MIN);
                let maximum = self.maximum.unwrap_or(f64::MAX);
                let value = if minimum <= maximum {
                    value.clamp(minimum, maximum)
                } else {
                    value
                };
                serde_json::Number::from_f64(value)
                    .map(Value::Number)
                    .ok_or_else(|| format!("{}には数値を入力してください", self.label))
            }
            ObjectControlKind::Text
            | ObjectControlKind::Path
            | ObjectControlKind::Color
            | ObjectControlKind::Font => Ok(Value::String(input.to_owned())),
            _ => Err(format!("{}はテキストとして編集できません", self.label)),
        }
    }

    pub fn option_value(&self, index: usize) -> Option<Value> {
        self.options.get(index).map(|option| option.value.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectEffect {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub selected: bool,
    pub removable: bool,
    pub controls: Vec<ObjectControl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectSettings {
    pub clip_id: i32,
    pub clip_label: String,
    pub effects: Vec<ObjectEffect>,
}

pub fn project_object_settings(
    document: &ProjectDocument,
    clip: &ClipDocument,
    current_scene: i32,
    playhead: i32,
    catalog: &EffectCatalog,
    effect_is_selected: impl Fn(usize) -> bool,
) -> ObjectSettings {
    let relative_frame = playhead
        .saturating_sub(clip.start)
        .clamp(0, clip.duration.max(0));
    let effects = clip
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            let metadata = catalog.find(&effect.id);
            ObjectEffect {
                index,
                id: effect.id.clone(),
                name: effect_name(effect, metadata),
                enabled: effect.enabled,
                selected: effect_is_selected(index),
                removable: effect_is_removable(catalog, clip, index),
                controls: metadata.map_or_else(Vec::new, |metadata| {
                    effect_controls(
                        metadata,
                        effect,
                        document,
                        current_scene,
                        relative_frame,
                        clip.duration,
                    )
                }),
            }
        })
        .collect();
    let clip_label = clip
        .effects
        .first()
        .map(|effect| effect_name(effect, catalog.find(&effect.id)))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| clip.clip_type.clone());
    ObjectSettings {
        clip_id: clip.id,
        clip_label,
        effects,
    }
}

pub(crate) fn replace_value_payload(original: &Value, value: Value) -> Value {
    if let Some(mut object) = original
        .as_object()
        .filter(|object| object.contains_key("$aviqtlType"))
        .cloned()
    {
        let value = match object.get("$aviqtlType").and_then(Value::as_str) {
            Some("int" | "integer") => value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| {
                    Value::from(value.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64)
                })
                .unwrap_or(value),
            _ => value,
        };
        object.insert("value".to_owned(), value);
        Value::Object(object)
    } else {
        value
    }
}

fn effect_controls(
    metadata: &EffectMetadata,
    effect: &EffectDocument,
    document: &ProjectDocument,
    current_scene: i32,
    relative_frame: i32,
    clip_duration: i32,
) -> Vec<ObjectControl> {
    metadata
        .ui
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let definition = value.as_object()?;
            let source_kind = definition.get("type")?.as_str()?.to_owned();
            let kind = control_kind(&source_kind);
            let param = definition
                .get("param")
                .or_else(|| definition.get("name"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let label = definition
                .get("label")
                .and_then(Value::as_str)
                .or(param.as_deref())
                .unwrap_or("パラメータ")
                .to_owned();
            let fallback = param
                .as_deref()
                .and_then(|name| {
                    effect
                        .params
                        .get(name)
                        .or_else(|| metadata.params.get(name))
                })
                .cloned()
                .unwrap_or(Value::Null);
            let track = param
                .as_deref()
                .and_then(|name| effect.keyframes.as_ref()?.get(name));
            let value =
                evaluate_keyframe_track(track, &fallback, clip_duration.max(0), relative_frame);
            let keyframes = inspect_keyframe_track(track, &fallback, clip_duration.max(0));
            let (interval_start, interval_end) =
                keyframe_interval(&keyframes, relative_frame, clip_duration.max(0));
            let start_value =
                evaluate_keyframe_track(track, &fallback, clip_duration.max(0), interval_start);
            let end_value =
                evaluate_keyframe_track(track, &fallback, clip_duration.max(0), interval_end);
            let start_interpolation = if track.is_some() {
                keyframes
                    .iter()
                    .find(|point| point.frame == interval_start)
                    .map_or_else(|| "linear".to_owned(), |point| point.interpolation.clone())
            } else {
                "constant".to_owned()
            };
            let source_property = definition
                .get("sourceProperty")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let exclude_current_scene = definition
                .get("excludeCurrentScene")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let options = if source_kind == "combo" && source_property == "scenes" {
                document
                    .scenes
                    .iter()
                    .filter(|scene| !exclude_current_scene || scene.id != current_scene)
                    .map(|scene| ObjectControlOption {
                        value: Value::from(scene.id),
                        label: scene.name.clone(),
                    })
                    .collect()
            } else {
                definition
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(control_option)
                    .collect()
            };
            Some(ObjectControl {
                kind,
                source_kind,
                param,
                label,
                minimum: definition.get("min").and_then(Value::as_f64),
                maximum: definition.get("max").and_then(Value::as_f64),
                step: definition.get("step").and_then(Value::as_f64),
                decimals: definition
                    .get("decimals")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok()),
                unit: definition
                    .get("unit")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                filter: definition
                    .get("filter")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                disabled: definition
                    .get("disabledByVideoLink")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && effect_source_is_video(effect)
                    && effect.params.get("linkedVideo").and_then(Value::as_bool) == Some(true),
                keyframed: track.is_some(),
                value,
                relative_frame,
                clip_duration: clip_duration.max(0),
                interval_start,
                interval_end,
                start_value,
                end_value,
                start_interpolation,
                keyframes,
                options,
            })
        })
        .collect()
}

fn keyframe_interval(points: &[KeyframePoint], current_frame: i32, duration: i32) -> (i32, i32) {
    let duration = duration.max(0);
    if points.is_empty() {
        return (0, duration);
    }
    if current_frame >= duration {
        let start = points
            .iter()
            .rev()
            .find(|point| point.frame < duration)
            .map_or(0, |point| point.frame);
        return (start, duration);
    }
    let Some((index, start)) = points
        .iter()
        .enumerate()
        .rev()
        .find(|(_, point)| point.frame <= current_frame)
    else {
        return (0, points[0].frame.min(duration));
    };
    let end = points
        .get(index + 1)
        .map_or(duration, |point| point.frame.min(duration));
    (start.frame, end)
}

fn control_kind(kind: &str) -> ObjectControlKind {
    match kind {
        "float" | "number" | "slider" | "spinner" => ObjectControlKind::Number,
        "int" | "integer" | "scene_id" => ObjectControlKind::Integer,
        "bool" | "boolean" => ObjectControlKind::Boolean,
        "string" | "text" => ObjectControlKind::Text,
        "path" | "file" => ObjectControlKind::Path,
        "color" | "colour" => ObjectControlKind::Color,
        "font" => ObjectControlKind::Font,
        "enum" | "combo" => ObjectControlKind::Choice,
        "header" => ObjectControlKind::Header,
        _ => ObjectControlKind::Unsupported,
    }
}

fn control_option(value: &Value) -> Option<ObjectControlOption> {
    if let Some(label) = value.as_str() {
        return Some(ObjectControlOption {
            value: Value::String(label.to_owned()),
            label: label.to_owned(),
        });
    }
    let object = value.as_object()?;
    let option_value = object
        .get("value")
        .or_else(|| object.get("id"))
        .or_else(|| object.get("label"))?
        .clone();
    let label = object
        .get("label")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| value_payload(&option_value).to_string());
    Some(ObjectControlOption {
        value: option_value,
        label,
    })
}

fn effect_name(effect: &EffectDocument, metadata: Option<&EffectMetadata>) -> String {
    if !effect.name.is_empty() {
        effect.name.clone()
    } else if let Some(name) = metadata
        .map(|metadata| metadata.name.as_str())
        .filter(|name| !name.is_empty())
    {
        name.to_owned()
    } else {
        effect.id.clone()
    }
}

fn effect_is_removable(catalog: &EffectCatalog, clip: &ClipDocument, effect_index: usize) -> bool {
    clip.effects.get(effect_index).is_some_and(|effect| {
        catalog
            .find(&effect.id)
            .map(|metadata| metadata.kind == "effect")
            .unwrap_or_else(|| {
                !((effect_index == 0 && effect.id == "transform") || effect.id == clip.clip_type)
            })
    })
}

fn effect_source_is_video(effect: &EffectDocument) -> bool {
    effect
        .params
        .get("source")
        .map(value_payload)
        .and_then(Value::as_str)
        .and_then(|source| Path::new(source).extension())
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["mp4", "mov", "avi", "mkv", "webm", "wmv"]
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn value_payload(value: &Value) -> &Value {
    value
        .as_object()
        .filter(|object| object.contains_key("$aviqtlType"))
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::{TimelineState, parse_effect_metadata};
    use serde_json::json;

    #[test]
    fn controls_preserve_metadata_order_types_options_and_ranges() {
        let metadata =
            parse_effect_metadata(include_bytes!("../../../ui/qml/effects/transform.json"))
                .expect("transform metadata");
        let effect = EffectDocument {
            id: "transform".to_owned(),
            name: String::new(),
            enabled: true,
            params: metadata.params.clone(),
            keyframes: None,
            extra: Default::default(),
        };
        let document = TimelineState::from_json(
            br#"{"version":3,"scenes":[{"id":1,"name":"Root","duration":300}],"clips":[]}"#,
        )
        .expect("document")
        .snapshot();
        let controls = effect_controls(&metadata, &effect, &document, 1, 0, 100);

        assert_eq!(
            controls
                .first()
                .and_then(|control| control.param.as_deref()),
            Some("x")
        );
        assert_eq!(
            controls.last().and_then(|control| control.param.as_deref()),
            Some("backfaceVisible")
        );
        let blend = controls
            .iter()
            .find(|control| control.param.as_deref() == Some("blendMode"))
            .expect("blend mode");
        assert_eq!(blend.kind, ObjectControlKind::Choice);
        assert_eq!(blend.options[0].label, "通常");
        assert_eq!(blend.options[0].value, json!("通常"));
    }

    #[test]
    fn scene_combo_is_resolved_from_the_live_document() {
        let metadata =
            parse_effect_metadata(include_bytes!("../../../ui/qml/objects/SceneObject.json"))
                .expect("scene metadata");
        let effect = EffectDocument {
            id: "scene".to_owned(),
            name: String::new(),
            enabled: true,
            params: metadata.params.clone(),
            keyframes: None,
            extra: Default::default(),
        };
        let document = TimelineState::from_json(
            br#"{"version":3,"scenes":[{"id":1,"name":"Root","duration":300},{"id":2,"name":"Nested","duration":120}],"clips":[]}"#,
        )
        .expect("document")
        .snapshot();
        let controls = effect_controls(&metadata, &effect, &document, 1, 0, 100);
        let scene = controls
            .iter()
            .find(|control| control.param.as_deref() == Some("targetSceneId"))
            .expect("scene combo");

        assert_eq!(scene.options.len(), 1);
        assert_eq!(scene.options[0].label, "Nested");
        assert_eq!(scene.options[0].value, json!(2));
    }

    #[test]
    fn text_parsing_and_payload_replacement_keep_qt_value_contracts() {
        let integer = ObjectControl {
            kind: ObjectControlKind::Integer,
            source_kind: "int".to_owned(),
            param: Some("quality".to_owned()),
            label: "Quality".to_owned(),
            minimum: Some(0.0),
            maximum: Some(10.0),
            step: Some(1.0),
            decimals: Some(0),
            unit: String::new(),
            filter: String::new(),
            disabled: false,
            keyframed: false,
            value: json!(0),
            relative_frame: 0,
            clip_duration: 100,
            interval_start: 0,
            interval_end: 100,
            start_value: json!(0),
            end_value: json!(0),
            start_interpolation: "constant".to_owned(),
            keyframes: Vec::new(),
            options: Vec::new(),
        };
        assert_eq!(integer.parse_text("12").expect("integer"), json!(10));
        assert!(integer.parse_text("4.6").is_err());
        let mut text = integer.clone();
        text.kind = ObjectControlKind::Text;
        assert_eq!(
            text.parse_text("  keep me  ").expect("text"),
            json!("  keep me  ")
        );

        let original = json!({"$aviqtlType":"int","value":0,"extension":7});
        let edited = replace_value_payload(&original, json!(4.6));
        assert_eq!(edited["$aviqtlType"], "int");
        assert_eq!(edited["value"], 5);
        assert_eq!(edited["extension"], 7);
    }

    #[test]
    fn projection_uses_current_keyframe_values_and_removable_metadata() {
        let state = TimelineState::from_json(
            br#"{
                "version":3,
                "scenes":[{"id":1,"name":"Root","duration":300}],
                "clips":[{
                    "id":7,"sceneId":1,"type":"rect","start":10,"duration":100,"layer":0,
                    "effects":[
                        {"id":"rect","name":"","params":{"width":100}},
                        {"id":"blur","name":"","params":{"size":0,"quality":1},"keyframes":{"size":[{"frame":0,"value":0},{"frame":20,"value":20}]}}
                    ]
                }]
            }"#,
        )
        .expect("project");
        let document = state.snapshot();
        let clip = &document.clips[0];
        let (catalog, _) = EffectCatalog::load();
        let projection =
            project_object_settings(&document, clip, 1, 20, &catalog, |index| index == 1);

        assert!(!projection.effects[0].removable);
        assert!(projection.effects[1].removable);
        assert!(projection.effects[1].selected);
        let size = projection.effects[1]
            .controls
            .iter()
            .find(|control| control.param.as_deref() == Some("size"))
            .expect("size");
        assert!(size.keyframed);
        assert_eq!(size.number_value(), 10.0);
        assert_eq!((size.interval_start, size.interval_end), (0, 20));
        assert_eq!(size.start_value, json!(0));
        assert_eq!(size.end_value, json!(20));
        assert_eq!(size.keyframes.len(), 2);
    }
}
