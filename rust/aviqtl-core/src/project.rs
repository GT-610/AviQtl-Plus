use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, STATUS_UNSUPPORTED_VERSION, ranges_overlap, slice_is_valid,
};
use crate::policy::canonical_playback_mode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};

const MIN_PROJECT_VERSION: i32 = 1;
const MAX_PROJECT_VERSION: i32 = 3;
// Keep in sync with AviQtl::kDefaultWidth in core/include/constants.hpp.
const DEFAULT_WIDTH: i32 = 1920;
// Keep in sync with AviQtl::kDefaultHeight in core/include/constants.hpp.
const DEFAULT_HEIGHT: i32 = 1080;
// Keep in sync with AviQtl::kDefaultSampleRate in core/include/constants.hpp.
const DEFAULT_SAMPLE_RATE: i32 = 48_000;
// Keep in sync with AviQtl::kDefaultTotalFrames in core/include/constants.hpp.
const DEFAULT_TOTAL_FRAMES: i32 = 300;
// Keep in sync with AviQtl::kDefaultFps in core/include/constants.hpp.
pub(crate) const DEFAULT_FPS: f64 = 60.0;
const MAX_DIMENSION: i32 = 32_768;
const MAX_FPS: f64 = 1_000.0;
const MAX_SAMPLE_RATE: i32 = 192_000;
const MAX_GRID_BPM: f64 = 1_000.0;
const MAX_GRID_OFFSET: f64 = 86_400.0;
const MAX_GRID_INTERVAL: i32 = 1_000_000;
const MAX_GRID_SUBDIVISION: i32 = 128;
const MAX_MAGNETIC_SNAP_RANGE: i32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectError {
    InvalidJson,
    UnsupportedVersion,
}

pub(crate) type ExtraFields = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectDocument {
    pub(crate) version: i32,
    pub(crate) settings: ProjectSettings,
    pub(crate) scenes: Vec<SceneDocument>,
    pub(crate) clips: Vec<ClipDocument>,
    #[serde(flatten)]
    pub(crate) extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectSettings {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) fps: f64,
    #[serde(rename = "sampleRate")]
    pub(crate) sample_rate: i32,
    #[serde(flatten)]
    pub(crate) extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SceneDocument {
    pub(crate) id: i32,
    pub(crate) name: String,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) fps: f64,
    pub(crate) start: i32,
    pub(crate) duration: i32,
    #[serde(rename = "nestedDuration")]
    pub(crate) nested_duration: i32,
    #[serde(rename = "lockedLayers")]
    pub(crate) locked_layers: Vec<i32>,
    #[serde(rename = "hiddenLayers")]
    pub(crate) hidden_layers: Vec<i32>,
    #[serde(rename = "gridMode")]
    pub(crate) grid_mode: String,
    #[serde(rename = "gridBpm")]
    pub(crate) grid_bpm: f64,
    #[serde(rename = "gridOffset")]
    pub(crate) grid_offset: f64,
    #[serde(rename = "gridInterval")]
    pub(crate) grid_interval: i32,
    #[serde(rename = "gridSubdivision")]
    pub(crate) grid_subdivision: i32,
    #[serde(rename = "enableSnap")]
    pub(crate) enable_snap: bool,
    #[serde(rename = "magneticSnapRange")]
    pub(crate) magnetic_snap_range: i32,
    #[serde(flatten)]
    pub(crate) extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ClipDocument {
    pub(crate) id: i32,
    #[serde(rename = "sceneId")]
    pub(crate) scene_id: i32,
    #[serde(rename = "type")]
    pub(crate) clip_type: String,
    pub(crate) start: i32,
    pub(crate) duration: i32,
    pub(crate) layer: i32,
    #[serde(rename = "clipByUpperObject")]
    pub(crate) clip_by_upper_object: bool,
    pub(crate) params: Map<String, Value>,
    #[serde(rename = "audioPlugins")]
    pub(crate) audio_plugins: Vec<AudioPluginDocument>,
    pub(crate) effects: Vec<EffectDocument>,
    #[serde(flatten)]
    pub(crate) extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AudioPluginDocument {
    pub(crate) id: String,
    pub(crate) enabled: bool,
    pub(crate) params: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keyframes: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub(crate) extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct EffectDocument {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) params: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keyframes: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub(crate) extra: ExtraFields,
}

fn integer(value: Option<&Value>, fallback: i32) -> i32 {
    let Some(number) = value.and_then(Value::as_number) else {
        return fallback;
    };
    if let Some(value) = number.as_i64().and_then(|value| i32::try_from(value).ok()) {
        return value;
    }
    let Some(value) = number.as_f64() else {
        return fallback;
    };
    if value.is_finite()
        && value.fract() == 0.0
        && value >= f64::from(i32::MIN)
        && value <= f64::from(i32::MAX)
    {
        value as i32
    } else {
        fallback
    }
}

fn floating(value: Option<&Value>, fallback: f64) -> f64 {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}

fn boolean(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

fn string(value: Option<&Value>, fallback: &str) -> String {
    value.and_then(Value::as_str).unwrap_or(fallback).to_owned()
}

fn object(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn set_integer(map: &mut Map<String, Value>, key: &str, value: i32) {
    map.insert(key.to_owned(), Value::Number(Number::from(value)));
}

fn set_floating(map: &mut Map<String, Value>, key: &str, value: f64) {
    let value = if value.is_finite() { value } else { 0.0 };
    let number = Number::from_f64(value).unwrap_or(Number::from(0));
    map.insert(key.to_owned(), Value::Number(number));
}

fn set_boolean(map: &mut Map<String, Value>, key: &str, value: bool) {
    map.insert(key.to_owned(), Value::Bool(value));
}

fn set_string(map: &mut Map<String, Value>, key: &str, value: String) {
    map.insert(key.to_owned(), Value::String(value));
}

fn bounded_positive(value: i32, maximum: i32, fallback: i32) -> i32 {
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

fn normalize_layers(value: Option<&Value>) -> Value {
    let mut layers = BTreeSet::new();
    for layer in array(value) {
        let index = integer(Some(&layer), -1);
        if (0..=127).contains(&index) {
            layers.insert(index);
        }
    }
    Value::Array(
        layers
            .into_iter()
            .map(|layer| Value::Number(Number::from(layer)))
            .collect(),
    )
}

fn normalize_settings(root: &mut Map<String, Value>) -> (i32, i32, f64, i32) {
    let mut settings = object(root.get("settings"));
    let width = bounded_positive(
        integer(settings.get("width"), DEFAULT_WIDTH),
        MAX_DIMENSION,
        DEFAULT_WIDTH,
    );
    let height = bounded_positive(
        integer(settings.get("height"), DEFAULT_HEIGHT),
        MAX_DIMENSION,
        DEFAULT_HEIGHT,
    );
    let fps = bounded_positive_f64(
        floating(settings.get("fps"), DEFAULT_FPS),
        MAX_FPS,
        DEFAULT_FPS,
    );
    let sample_rate = bounded_positive(
        integer(settings.get("sampleRate"), DEFAULT_SAMPLE_RATE),
        MAX_SAMPLE_RATE,
        DEFAULT_SAMPLE_RATE,
    );

    set_integer(&mut settings, "width", width);
    set_integer(&mut settings, "height", height);
    set_floating(&mut settings, "fps", fps);
    set_integer(&mut settings, "sampleRate", sample_rate);
    root.insert("settings".to_owned(), Value::Object(settings));
    (width, height, fps, sample_rate)
}

fn normalize_scenes(
    root: &mut Map<String, Value>,
    version: i32,
    project_width: i32,
    project_height: i32,
    project_fps: f64,
) {
    let scenes = array(root.get("scenes"))
        .into_iter()
        .map(|scene| {
            let mut scene = object(Some(&scene));
            let id = integer(scene.get("id"), 0);
            let name = string(scene.get("name"), "");
            let width = bounded_positive(
                integer(scene.get("width"), project_width),
                MAX_DIMENSION,
                project_width,
            );
            let height = bounded_positive(
                integer(scene.get("height"), project_height),
                MAX_DIMENSION,
                project_height,
            );
            let fps = bounded_positive_f64(
                floating(scene.get("fps"), project_fps),
                MAX_FPS,
                project_fps,
            );
            let total_frames = bounded_positive(
                integer(scene.get("duration"), DEFAULT_TOTAL_FRAMES),
                i32::MAX,
                DEFAULT_TOTAL_FRAMES,
            );
            let nested_duration = if version >= 3 {
                integer(scene.get("nestedDuration"), 0).max(0)
            } else {
                0
            };
            let grid_bpm = if version >= 3 {
                bounded_positive_f64(floating(scene.get("gridBpm"), 120.0), MAX_GRID_BPM, 120.0)
            } else {
                120.0
            };
            let grid_offset = if version >= 3 {
                let value = floating(scene.get("gridOffset"), 0.0);
                if !(0.0..=MAX_GRID_OFFSET).contains(&value) {
                    0.0
                } else {
                    value
                }
            } else {
                0.0
            };
            let grid_interval = if version >= 3 {
                bounded_positive(
                    integer(scene.get("gridInterval"), 10),
                    MAX_GRID_INTERVAL,
                    10,
                )
            } else {
                10
            };
            let grid_subdivision = if version >= 3 {
                bounded_positive(
                    integer(scene.get("gridSubdivision"), 4),
                    MAX_GRID_SUBDIVISION,
                    4,
                )
            } else {
                4
            };
            let magnetic_snap_range = if version >= 3 {
                bounded_positive(
                    integer(scene.get("magneticSnapRange"), 10),
                    MAX_MAGNETIC_SNAP_RANGE,
                    10,
                )
            } else {
                10
            };
            let start = integer(scene.get("start"), 0);

            set_integer(&mut scene, "id", id);
            set_string(&mut scene, "name", name);
            set_integer(&mut scene, "width", width);
            set_integer(&mut scene, "height", height);
            set_floating(&mut scene, "fps", fps);
            set_integer(&mut scene, "start", start);
            set_integer(&mut scene, "duration", total_frames);
            set_integer(&mut scene, "nestedDuration", nested_duration);
            scene.insert(
                "lockedLayers".to_owned(),
                if version >= 3 {
                    normalize_layers(scene.get("lockedLayers"))
                } else {
                    Value::Array(Vec::new())
                },
            );
            scene.insert(
                "hiddenLayers".to_owned(),
                if version >= 3 {
                    normalize_layers(scene.get("hiddenLayers"))
                } else {
                    Value::Array(Vec::new())
                },
            );
            let grid_mode = if version >= 3 {
                string(scene.get("gridMode"), "Auto")
            } else {
                "Auto".to_owned()
            };
            set_string(&mut scene, "gridMode", grid_mode);
            set_floating(&mut scene, "gridBpm", grid_bpm);
            set_floating(&mut scene, "gridOffset", grid_offset);
            set_integer(&mut scene, "gridInterval", grid_interval);
            set_integer(&mut scene, "gridSubdivision", grid_subdivision);
            let enable_snap = version < 3 || boolean(scene.get("enableSnap"), true);
            set_boolean(&mut scene, "enableSnap", enable_snap);
            set_integer(&mut scene, "magneticSnapRange", magnetic_snap_range);
            Value::Object(scene)
        })
        .collect();
    root.insert("scenes".to_owned(), Value::Array(scenes));
}

fn normalize_audio_plugins(value: Option<&Value>) -> Value {
    let plugins = array(value)
        .into_iter()
        .filter_map(|plugin| {
            let mut plugin = object(Some(&plugin));
            let id = string(plugin.get("id"), "");
            if id.is_empty() {
                return None;
            }
            let enabled = boolean(plugin.get("enabled"), true);
            set_string(&mut plugin, "id", id);
            set_boolean(&mut plugin, "enabled", enabled);
            plugin.insert(
                "params".to_owned(),
                Value::Object(object(plugin.get("params"))),
            );
            if plugin.contains_key("keyframes") {
                plugin.insert(
                    "keyframes".to_owned(),
                    Value::Object(object(plugin.get("keyframes"))),
                );
            }
            Some(Value::Object(plugin))
        })
        .collect();
    Value::Array(plugins)
}

fn normalize_playback_value(value: &mut Value) {
    if let Some(canonical) = value.as_str().and_then(canonical_playback_mode) {
        *value = Value::String(canonical.to_owned());
    }
}

fn normalize_playback_track(track: &mut Value) {
    let normalize_point = |point: &mut Value| {
        if let Some(value) = point
            .as_object_mut()
            .and_then(|point| point.get_mut("value"))
        {
            normalize_playback_value(value);
        }
    };
    if let Some(points) = track.as_array_mut() {
        for point in points {
            normalize_point(point);
        }
        return;
    }
    let Some(track) = track.as_object_mut() else {
        return;
    };
    if let Some(start) = track.get_mut("start") {
        normalize_point(start);
    }
    if let Some(points) = track.get_mut("points").and_then(Value::as_array_mut) {
        for point in points {
            normalize_point(point);
        }
    }
}

fn normalize_playback_parameters(effect_id: &str, params: &mut Map<String, Value>) {
    if !matches!(effect_id, "audio" | "video") {
        return;
    }
    if let Some(value) = params.get_mut("playMode") {
        normalize_playback_value(value);
    }
}

fn normalize_effects(value: Option<&Value>) -> Value {
    let effects = array(value)
        .into_iter()
        .map(|effect| {
            let mut effect = object(Some(&effect));
            let id = match string(effect.get("id"), "").as_str() {
                "camera" => "camera_control".to_owned(),
                id => id.to_owned(),
            };
            let name = string(effect.get("name"), "");
            let enabled = boolean(effect.get("enabled"), true);
            let is_media_effect = matches!(id.as_str(), "audio" | "video");
            set_string(&mut effect, "id", id.clone());
            set_string(&mut effect, "name", name);
            set_boolean(&mut effect, "enabled", enabled);
            let mut params = object(effect.get("params"));
            normalize_playback_parameters(&id, &mut params);
            effect.insert("params".to_owned(), Value::Object(params));
            if effect.contains_key("keyframes") {
                let mut keyframes = object(effect.get("keyframes"));
                if is_media_effect && let Some(track) = keyframes.get_mut("playMode") {
                    normalize_playback_track(track);
                }
                effect.insert("keyframes".to_owned(), Value::Object(keyframes));
            }
            Value::Object(effect)
        })
        .collect();
    Value::Array(effects)
}

fn normalize_clips(root: &mut Map<String, Value>) {
    let clips = array(root.get("clips"))
        .into_iter()
        .map(|clip| {
            let mut clip = object(Some(&clip));
            let clip_type = match string(clip.get("type"), "").as_str() {
                "camera" => "camera_control".to_owned(),
                value => value.to_owned(),
            };
            let id = integer(clip.get("id"), 0);
            let scene_id = integer(clip.get("sceneId"), 0);
            let start = integer(clip.get("start"), 0);
            let duration = integer(clip.get("duration"), 0);
            let layer = integer(clip.get("layer"), 0).clamp(0, 127);
            let clip_by_upper_object = boolean(clip.get("clipByUpperObject"), false);
            set_integer(&mut clip, "id", id);
            set_integer(&mut clip, "sceneId", scene_id);
            set_string(&mut clip, "type", clip_type);
            set_integer(&mut clip, "start", start);
            set_integer(&mut clip, "duration", duration);
            set_integer(&mut clip, "layer", layer);
            set_boolean(&mut clip, "clipByUpperObject", clip_by_upper_object);
            clip.insert(
                "params".to_owned(),
                Value::Object(object(clip.get("params"))),
            );
            clip.insert(
                "audioPlugins".to_owned(),
                normalize_audio_plugins(clip.get("audioPlugins")),
            );
            clip.insert("effects".to_owned(), normalize_effects(clip.get("effects")));
            Value::Object(clip)
        })
        .collect();
    root.insert("clips".to_owned(), Value::Array(clips));
}

pub(crate) fn parse_project_document(input: &[u8]) -> Result<ProjectDocument, ProjectError> {
    let mut root: Value = serde_json::from_slice(input).map_err(|_| ProjectError::InvalidJson)?;
    {
        let root = root.as_object_mut().ok_or(ProjectError::InvalidJson)?;
        let version = integer(root.get("version"), MIN_PROJECT_VERSION);
        if !(MIN_PROJECT_VERSION..=MAX_PROJECT_VERSION).contains(&version) {
            return Err(ProjectError::UnsupportedVersion);
        }
        set_integer(root, "version", version);
        let (width, height, fps, _) = normalize_settings(root);
        normalize_scenes(root, version, width, height, fps);
        normalize_clips(root);
    }
    serde_json::from_value(root).map_err(|_| ProjectError::InvalidJson)
}

fn normalize_project_json(input: &[u8]) -> Result<Vec<u8>, ProjectError> {
    let document = parse_project_document(input)?;
    serde_json::to_vec(&document).map_err(|_| ProjectError::InvalidJson)
}

/// Returns the project file-format version emitted by the Rust document core.
#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_project_current_version() -> i32 {
    MAX_PROJECT_VERSION
}

/// Parses and normalizes a project document into caller-owned UTF-8 JSON storage.
///
/// A null output with zero capacity is a size query. In that mode, or when the
/// provided buffer is too small, `output_length` receives the required byte count
/// and `STATUS_BUFFER_TOO_SMALL` is returned.
///
/// # Safety
///
/// Non-zero lengths require valid byte ranges. `output_length` must point to writable,
/// aligned storage. Input, output, and output-length ranges must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_project_normalize_json(
    input: *const u8,
    input_length: usize,
    output: *mut u8,
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
        ranges_overlap(input, input_length, output, output_capacity),
        ranges_overlap(input, input_length, output_length, 1),
        ranges_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }

    let input = if input_length == 0 {
        &[]
    } else {
        // SAFETY: The input range was validated and all output overlap was rejected.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    let normalized = match normalize_project_json(input) {
        Ok(normalized) => normalized,
        Err(ProjectError::InvalidJson) => return STATUS_INVALID_JSON,
        Err(ProjectError::UnsupportedVersion) => return STATUS_UNSUPPORTED_VERSION,
    };
    // SAFETY: The output-length pointer was validated and checked for overlap.
    unsafe { output_length.write(normalized.len()) };
    if output_capacity < normalized.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    let output = if output_capacity == 0 {
        &mut []
    } else {
        // SAFETY: The output range was validated and checked against every input range.
        unsafe { std::slice::from_raw_parts_mut(output, output_capacity) }
    };
    output[..normalized.len()].copy_from_slice(&normalized);
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(input: &str) -> Value {
        let bytes = normalize_project_json(input.as_bytes()).expect("valid project document");
        serde_json::from_slice(&bytes).expect("normalized JSON")
    }

    #[test]
    fn normalizes_versions_settings_scenes_and_clips() {
        let normalized = normalize(
            r#"{
                "version": 1,
                "settings": {"width": 0, "height": 50000, "fps": 0, "sampleRate": 500000},
                "scenes": [{"id": 4, "duration": -1, "lockedLayers": [-1, 2, 2, 200]}],
                "clips": [{"id": 8, "type": "camera", "layer": 500,
                    "audioPlugins": [{"id": ""}, {"id": "gain"}],
                    "effects": [{"id": "camera"}]}]
            }"#,
        );
        assert_eq!(normalized["version"], 1);
        assert_eq!(normalized["settings"]["width"], DEFAULT_WIDTH);
        assert_eq!(normalized["settings"]["height"], DEFAULT_HEIGHT);
        assert_eq!(normalized["settings"]["fps"], DEFAULT_FPS);
        assert_eq!(normalized["settings"]["sampleRate"], DEFAULT_SAMPLE_RATE);
        assert_eq!(normalized["scenes"][0]["duration"], DEFAULT_TOTAL_FRAMES);
        assert_eq!(normalized["scenes"][0]["gridBpm"], 120.0);
        assert_eq!(
            normalized["scenes"][0]["lockedLayers"],
            serde_json::json!([])
        );
        assert_eq!(normalized["clips"][0]["type"], "camera_control");
        assert_eq!(normalized["clips"][0]["layer"], 127);
        assert_eq!(
            normalized["clips"][0]["audioPlugins"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(normalized["clips"][0]["effects"][0]["id"], "camera_control");
    }

    #[test]
    fn typed_document_preserves_extension_fields() {
        let input = r#"{
            "version": 3,
            "settings": {"width": 1920, "height": 1080, "fps": 60,
                "sampleRate": 48000, "settingsExtension": 1},
            "scenes": [{"id": 1, "name": "Scene", "sceneExtension": true}],
            "clips": [{"id": 2, "sceneId": 1, "type": "object", "start": 0,
                "duration": 10, "layer": 0, "clipExtension": "kept",
                "effects": [{"id": "object", "effectExtension": 7}]}],
            "documentExtension": {"owner": "plugin"}
        }"#;
        let document = parse_project_document(input.as_bytes()).expect("typed project document");
        assert_eq!(document.version, aviqtl_project_current_version());
        assert_eq!(document.scenes[0].id, 1);
        assert_eq!(document.clips[0].scene_id, 1);
        assert_eq!(document.extra["documentExtension"]["owner"], "plugin");

        let normalized = normalize(input);
        assert_eq!(normalized["settings"]["settingsExtension"], 1);
        assert_eq!(normalized["scenes"][0]["sceneExtension"], true);
        assert_eq!(normalized["clips"][0]["clipExtension"], "kept");
        assert_eq!(normalized["clips"][0]["effects"][0]["effectExtension"], 7);
    }

    #[test]
    fn normalizes_legacy_playback_modes_without_changing_labels() {
        let normalized = normalize(
            r#"{
                "version": 3,
                "settings": {},
                "scenes": [{"id": 1}],
                "clips": [{"id": 2, "sceneId": 1, "type": "audio", "effects": [
                    {"id": "audio", "params": {"playMode": "時間直接指定"},
                     "keyframes": {"playMode": {"start": {"frame": 0, "value": "開始時間＋再生速度"},
                         "points": [{"frame": 10, "value": "時間直接指定"}]}}},
                    {"id": "label", "params": {"playMode": "時間直接指定"}}
                ]}]
            }"#,
        );
        assert_eq!(
            normalized["clips"][0]["effects"][0]["params"]["playMode"],
            "direct"
        );
        assert_eq!(
            normalized["clips"][0]["effects"][0]["keyframes"]["playMode"]["start"]["value"],
            "normal"
        );
        assert_eq!(
            normalized["clips"][0]["effects"][0]["keyframes"]["playMode"]["points"][0]["value"],
            "direct"
        );
        assert_eq!(
            normalized["clips"][0]["effects"][1]["params"]["playMode"],
            "時間直接指定"
        );
    }

    #[test]
    fn rejects_invalid_roots_and_unsupported_versions() {
        assert_eq!(
            normalize_project_json(b"{broken"),
            Err(ProjectError::InvalidJson)
        );
        assert_eq!(
            normalize_project_json(b"[]"),
            Err(ProjectError::InvalidJson)
        );
        assert_eq!(
            normalize_project_json(br#"{"version": 4}"#),
            Err(ProjectError::UnsupportedVersion)
        );
    }

    #[test]
    fn ffi_supports_size_queries_and_rejects_overlap() {
        let input = br#"{"version":3,"settings":{},"scenes":[],"clips":[]}"#;
        let mut required = 0_usize;
        // SAFETY: Input and output-length storage are valid and disjoint.
        let status = unsafe {
            aviqtl_project_normalize_json(
                input.as_ptr(),
                input.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
            )
        };
        assert_eq!(status, STATUS_BUFFER_TOO_SMALL);
        assert!(required > 1);

        let mut undersized = vec![0xA5_u8; required - 1];
        let original_undersized = undersized.clone();
        let mut reported = 0_usize;
        // SAFETY: The non-empty output is valid and disjoint but deliberately too small.
        let status = unsafe {
            aviqtl_project_normalize_json(
                input.as_ptr(),
                input.len(),
                undersized.as_mut_ptr(),
                undersized.len(),
                &mut reported,
            )
        };
        assert_eq!(status, STATUS_BUFFER_TOO_SMALL);
        assert_eq!(reported, required);
        assert_eq!(undersized, original_undersized);

        let mut output = vec![0_u8; required];
        // SAFETY: All ranges are valid, writable where required, and disjoint.
        let status = unsafe {
            aviqtl_project_normalize_json(
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut required,
            )
        };
        assert_eq!(status, STATUS_OK);
        assert!(serde_json::from_slice::<Value>(&output).is_ok());

        let mut overlapping = input.to_vec();
        let mut ignored = 0_usize;
        // SAFETY: The deliberate overlap must be rejected before slices are constructed.
        let status = unsafe {
            aviqtl_project_normalize_json(
                overlapping.as_ptr(),
                overlapping.len(),
                overlapping.as_mut_ptr(),
                overlapping.len(),
                &mut ignored,
            )
        };
        assert_eq!(status, STATUS_OVERLAPPING_BUFFERS);
    }
}
