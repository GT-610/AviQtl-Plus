//! Writes project clips as an AviUtl ExEdit object file.
//!
//! This is the inverse of [`crate::exo_import`]. It writes the subset the importer understands
//! — text, image, video, audio objects with their transform and mapped effects — plus the
//! preserved placeholders, which are written back from the data the importer stored on them.
//! Clips whose type has no ExEdit equivalent are skipped and reported rather than written as
//! something AviUtl would misread.

use crate::effect_catalog::EffectCatalog;
use crate::exo_import::PLACEHOLDER_ID;
use crate::exo_mapping::{
    BLEND_MODES, DRAWING_ALIASES, DRAWING_TRACKS, FilterMapping, ObjectMapping, TrackMapping,
};
use aviqtl_rust_core::api::{ClipDocument, EffectDocument, ProjectDocument, SceneDocument};
use aviqtl_rust_core::exo::{ExoFile, ExoFilter, ExoHeader, ExoObject, TextEncoding, TrackValue};
use serde_json::Value;

/// Result of writing a project to an object file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    pub objects: usize,
    pub skipped: Vec<String>,
}

impl ExportReport {
    /// One-line summary suitable for the status bar.
    pub fn summary(&self) -> String {
        if self.skipped.is_empty() {
            format!("Exported {} object(s)", self.objects)
        } else {
            format!(
                "Exported {} object(s), skipped {}",
                self.objects,
                self.skipped.len()
            )
        }
    }
}

/// Builds an object file from one scene of `document`.
///
/// The header mirrors the scene, so "create a new project from this file" in AviUtl produces
/// the same resolution, frame rate, and length the scene has here.
pub fn export_scene(
    document: &ProjectDocument,
    scene: &SceneDocument,
    catalog: &EffectCatalog,
) -> (ExoFile, ExportReport) {
    let mut report = ExportReport::default();
    let mut objects = Vec::new();
    let mut clips = document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene.id)
        .collect::<Vec<_>>();
    // AviUtl stacks layers upwards and objects are read in file order, so write the lowest
    // layer first and keep the original order inside a layer.
    clips.sort_by_key(|clip| (clip.layer, clip.start, clip.id));

    for clip in clips {
        match export_clip(clip, catalog, &mut report) {
            Some(object) => objects.push(object),
            None => report.skipped.push(format!(
                "{} (layer {}, frame {})",
                clip.clip_type,
                clip.layer + 1,
                clip.start + 1
            )),
        }
    }
    report.objects = objects.len();

    let header = ExoHeader {
        width: scene.width,
        height: scene.height,
        rate: scene.fps,
        scale: 100,
        length: scene.duration.max(1),
        audio_rate: document.settings.sample_rate,
        audio_channels: 2,
        alpha: false,
        name: Some(scene.name.clone()).filter(|name| !name.is_empty()),
        extra: Vec::new(),
    };
    (
        ExoFile {
            // AviUtl reads object files with its ANSI APIs, so the file has to be in the
            // code page of the machine that will open it. GBK is the safe default for the
            // translation packs this feature targets; UTF-8 remains readable by us.
            encoding: TextEncoding::Gbk,
            header,
            objects,
        },
        report,
    )
}

fn export_clip(
    clip: &ClipDocument,
    catalog: &EffectCatalog,
    report: &mut ExportReport,
) -> Option<ExoObject> {
    // The object's own filter is the clip-type effect, or the placeholder that stands in for
    // an object this build cannot render. Either way it is written once, first.
    let object_effect = clip
        .effects
        .iter()
        .find(|effect| effect.id == clip.clip_type && effect.id != PLACEHOLDER_ID)
        .or_else(|| {
            // A placeholder marks the object it stands for; the other placeholders in the
            // same clip are its effects.
            clip.effects.iter().find(|effect| {
                effect.params.get("exoObject").and_then(Value::as_bool) == Some(true)
            })
        })?;
    let first = if object_effect.id == PLACEHOLDER_ID {
        placeholder_filter(object_effect)?
    } else {
        object_filter_from_effect(&clip.clip_type, object_effect, catalog)?
    };

    let mut filters = vec![first];
    let transform = clip.effects.iter().find(|effect| effect.id == "transform");
    for effect in &clip.effects {
        if std::ptr::eq(effect, object_effect) || effect.id == "transform" {
            continue;
        }
        if effect.id == PLACEHOLDER_ID {
            if let Some(filter) = placeholder_filter(effect) {
                filters.push(filter);
            }
            continue;
        }
        match effect_filter(effect) {
            Some(filter) => filters.push(filter),
            None => report.skipped.push(format!("{} effect", effect.id)),
        }
    }
    if let Some(effect) = transform {
        filters.push(transform_filter(effect));
    }

    Some(ExoObject {
        start: clip.start.saturating_add(1),
        end: clip.start.saturating_add(clip.duration).max(1),
        layer: clip.layer.saturating_add(1),
        group: None,
        overlay: clip.clip_by_upper_object,
        clipping: false,
        // ExEdit writes `camera` on every object: `1` for the object that drives a camera,
        // `0` for everything else.
        camera: Some(clip.clip_type == "camera_control"),
        audio: false,
        chain: false,
        filters,
        extra: Vec::new(),
    })
}

fn object_filter_from_effect(
    clip_type: &str,
    effect: &EffectDocument,
    _catalog: &EffectCatalog,
) -> Option<ExoFilter> {
    let mapping = object_mapping_for_id(clip_type)?;
    let mut filter = ExoFilter::default();
    filter.set("_name", mapping.aliases[0]);
    if mapping.object_id == "text" {
        if let Some(font) = effect.params.get("fontFamily").and_then(Value::as_str)
            && !font.is_empty()
        {
            filter.set("font", font);
        }
        for parameter in &["fontSize", "letterSpacing", "lineSpacing"] {
            if let Some(number) = effect.params.get(*parameter).and_then(Value::as_f64) {
                let key = match *parameter {
                    "fontSize" => "大小",
                    "letterSpacing" => "spacing_x",
                    _ => "spacing_y",
                };
                filter.set(key, number_text(number));
            }
        }
        if let Some(text) = effect.params.get("text").and_then(Value::as_str) {
            filter.set("text", encode_utf16_hex(text));
        }
        for (param, key) in [("color", "color"), ("outlineColor", "color2")] {
            if let Some(color) = effect.params.get(param).and_then(Value::as_str) {
                filter.set(key, color.trim_start_matches('#'));
            }
        }
        for (param, key) in [("fontBold", "B"), ("fontItalic", "I")] {
            if effect.params.get(param).and_then(Value::as_bool) == Some(true) {
                filter.set(key, "1");
            }
        }
        if let Some(alignment) = effect.params.get("alignment").and_then(Value::as_i64) {
            let align = match alignment {
                4 => 2,
                2 => 4,
                8 => 8,
                // ExEdit's default, left alignment, is stored as zero.
                _ => 0,
            };
            filter.set("align", align.to_string());
        }
    }
    if mapping.media_path {
        let key = if mapping.object_id == "audio" {
            "source"
        } else {
            "path"
        };
        if let Some(path) = effect.params.get(key).and_then(Value::as_str)
            && !path.is_empty()
        {
            let mut blob = vec![0u8; 4];
            blob.extend_from_slice(path.as_bytes());
            blob.push(0);
            filter.set("_exdata", hex_encode(&blob));
        }
    }
    if !effect.enabled {
        filter.set("_disable", "1");
    }
    append_source_entries(&mut filter, effect);
    Some(filter)
}

/// Appends the source keys the importer preserved, skipping any the exporter already wrote.
///
/// ExEdit's own reader ignores keys it does not know, and it needs the positional markers and
/// per-object toggles this build does not interpret, so writing them back keeps a file usable
/// by AviUtl after a round trip.
fn append_source_entries(filter: &mut ExoFilter, effect: &EffectDocument) {
    for (key, value) in &effect.params {
        let Some(source_key) = key.strip_prefix("exo_") else {
            continue;
        };
        if source_key.starts_with("exo") {
            continue;
        }
        if filter.value(source_key).is_some() {
            continue;
        }
        if let Some(value) = value.as_str() {
            filter.set(source_key, value);
        }
    }
    if let Some(exdata) = effect.params.get("exoExdata").and_then(Value::as_str)
        && filter.value("_exdata").is_none()
    {
        filter.set("_exdata", exdata);
    }
}

fn object_mapping_for_id(clip_type: &str) -> Option<&'static ObjectMapping> {
    crate::exo_mapping::OBJECTS
        .iter()
        .find(|mapping| mapping.object_id == clip_type)
}

fn transform_filter(effect: &EffectDocument) -> ExoFilter {
    let mut filter = ExoFilter::default();
    filter.set("_name", DRAWING_ALIASES[0]);
    for mapping in DRAWING_TRACKS {
        write_track(&mut filter, effect, mapping);
    }
    if let Some(mode) = effect.params.get("blendMode").and_then(Value::as_str)
        && let Some(index) = BLEND_MODES.iter().position(|candidate| *candidate == mode)
    {
        filter.set("blend", index.to_string());
    }
    if effect.enabled {
        filter.remove("_disable");
    } else {
        filter.set("_disable", "1");
    }
    append_source_entries(&mut filter, effect);
    filter
}

/// Writes one trackbar, using the range form when the parameter is animated.
///
/// The motion label AviUtl stored is kept on the keyframe options, so an imported animation is
/// written back with the easing it came from instead of being flattened to a plain range.
fn write_track(filter: &mut ExoFilter, effect: &EffectDocument, mapping: &TrackMapping) {
    let Some(value) = effect.params.get(mapping.param).and_then(Value::as_f64) else {
        return;
    };
    let raw = (value - mapping.offset) / mapping.scale;
    let key = mapping.keys[0];
    let track = effect
        .keyframes
        .as_ref()
        .and_then(|tracks| tracks.get(mapping.param));
    match track.and_then(track_end_value) {
        Some(end) => {
            let end = (end - mapping.offset) / mapping.scale;
            let (motion, motion_parameter) = track.and_then(stored_motion).unzip();
            filter.set(
                key,
                TrackValue {
                    start: raw,
                    end: Some(end),
                    motion,
                    motion_parameter: motion_parameter
                        .flatten()
                        .and_then(|value| i32::try_from(value).ok()),
                }
                .to_text(),
            );
        }
        None => filter.set(key, number_text(raw)),
    }
}

/// Reads the ExEdit motion label an imported keyframe carries.
fn stored_motion(track: &Value) -> Option<(String, Option<i64>)> {
    let options = track
        .get("points")?
        .as_array()?
        .last()?
        .get("options")?
        .as_object()?;
    let motion = options.get("exoMotion")?.as_str()?.to_owned();
    Some((
        motion,
        options.get("exoMotionParameter").and_then(Value::as_i64),
    ))
}

/// Reads the last keyframe value of a track, which is the value at the object's end frame.
fn track_end_value(track: &Value) -> Option<f64> {
    let points = track.get("points")?.as_array()?;
    points
        .last()?
        .get("value")
        .and_then(Value::as_f64)
        .or_else(|| track.get("start")?.get("value")?.as_f64())
}

fn effect_filter(effect: &EffectDocument) -> Option<ExoFilter> {
    let mapping: &FilterMapping = filter_mapping_by_id(&effect.id)?;
    let mut filter = ExoFilter::default();
    filter.set("_name", mapping.aliases[0]);
    for mapping in mapping.tracks {
        write_track(&mut filter, effect, mapping);
    }
    for mapping in mapping.colors {
        if let Some(color) = effect.params.get(mapping.param).and_then(Value::as_str) {
            filter.set(mapping.keys[0], color.trim_start_matches('#'));
        }
    }
    for mapping in mapping.integers {
        if let Some(value) = effect.params.get(mapping.param).and_then(Value::as_f64) {
            let raw = (value - mapping.offset) / mapping.scale;
            filter.set(mapping.keys[0], (raw.round() as i64).to_string());
        }
    }
    for (keys, param) in mapping.flags {
        if effect.params.get(*param).and_then(Value::as_bool) == Some(true) {
            filter.set(keys[0], "1");
        }
    }
    if !effect.enabled {
        filter.set("_disable", "1");
    }
    append_source_entries(&mut filter, effect);
    Some(filter)
}

fn filter_mapping_by_id(effect_id: &str) -> Option<&'static FilterMapping> {
    crate::exo_mapping::FILTERS
        .iter()
        .find(|mapping| mapping.effect_id == effect_id)
}

/// Rebuilds the filter a placeholder was created from.
fn placeholder_filter(effect: &EffectDocument) -> Option<ExoFilter> {
    let mut filter = ExoFilter::default();
    let display = effect
        .params
        .get("exoDisplayName")
        .and_then(Value::as_str)
        .unwrap_or(&effect.name);
    filter.set("_name", display);
    if let Some(script) = effect.params.get("exoScript").and_then(Value::as_str) {
        filter.set("name", script);
    }
    if let Some(parameter) = effect
        .params
        .get("exoScriptParameter")
        .and_then(Value::as_str)
    {
        filter.set("param", parameter);
    }
    for group in ["exoTracks", "exoChecks"] {
        if let Some(values) = effect.params.get(group).and_then(Value::as_object) {
            for (key, value) in values {
                if let Some(value) = value.as_str() {
                    filter.set(key, value);
                }
            }
        }
    }
    if let Some(exdata) = effect.params.get("exoExdata").and_then(Value::as_str) {
        filter.set("_exdata", exdata);
    }
    for (key, value) in &effect.params {
        if let Some(key) = key.strip_prefix("exo_")
            && let Some(value) = value.as_str()
        {
            filter.set(key, value);
        }
    }
    if effect.params.get("exoDisabled").and_then(Value::as_bool) == Some(true) {
        filter.set("_disable", "1");
    }
    append_source_entries(&mut filter, effect);
    Some(filter)
}

/// Writes a number the way ExEdit does: one decimal for whole values.
fn number_text(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

fn encode_utf16_hex(text: &str) -> String {
    // The payload is a byte dump of a little-endian UTF-16 buffer, so each unit is written
    // low byte first.
    let mut encoded = String::new();
    for unit in text.encode_utf16() {
        encoded.push_str(&format!("{:02x}{:02x}", unit & 0xff, unit >> 8));
    }
    encoded
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// Reports whether `clip` can be written for AviUtl at all.
pub fn clip_is_exportable(clip: &ClipDocument) -> bool {
    object_mapping_for_id(&clip.clip_type).is_some()
        || clip
            .effects
            .iter()
            .any(|effect| effect.id == PLACEHOLDER_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keep_one_decimal_for_whole_values() {
        assert_eq!(number_text(0.0), "0.0");
        assert_eq!(number_text(100.0), "100.0");
        assert_eq!(number_text(1.25), "1.25");
    }

    #[test]
    fn text_is_written_as_utf16_hex() {
        assert_eq!(encode_utf16_hex("A"), "4100");
        assert_eq!(encode_utf16_hex("缓动"), "137fa852");
    }

    #[test]
    fn unknown_clip_types_are_not_exportable() {
        let clip = ClipDocument {
            id: 1,
            scene_id: 1,
            clip_type: "particle".to_owned(),
            start: 0,
            duration: 10,
            layer: 0,
            clip_by_upper_object: false,
            params: Default::default(),
            audio_plugins: Vec::new(),
            effects: Vec::new(),
            extra: Default::default(),
        };
        assert!(!clip_is_exportable(&clip));
    }
}
