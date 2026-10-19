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
    /// True when only the selected objects were written.
    pub selection_only: bool,
}

impl ExportReport {
    /// One-line summary suitable for the status bar.
    pub fn summary(&self) -> String {
        let scope = if self.selection_only {
            "selected object(s)"
        } else {
            "object(s)"
        };
        if self.skipped.is_empty() {
            format!("Exported {} {scope}", self.objects)
        } else {
            format!(
                "Exported {} {scope}, skipped {}",
                self.objects,
                self.skipped.len()
            )
        }
    }

    /// Multi-line report for a dialog, limited to `max_items` entries.
    pub fn details(&self, max_items: usize) -> String {
        let mut lines = vec![self.summary()];
        if self.skipped.is_empty() {
            return lines.join("\n");
        }
        lines.push(String::new());
        lines.extend(self.skipped.iter().take(max_items).cloned());
        if self.skipped.len() > max_items {
            lines.push(format!(
                "… and {} more",
                self.skipped.len().saturating_sub(max_items)
            ));
        }
        lines.join("\n")
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
    let clips = document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene.id)
        .collect::<Vec<_>>();
    build_file(document, scene, clips, catalog, false)
}

/// Builds an object file from the selected objects only.
///
/// This is what ExEdit's "export object file" does: it writes the selection, not the whole
/// scene, which is how users hand a group of objects to someone else or keep a reusable
/// fragment.
pub fn export_selection(
    document: &ProjectDocument,
    scene: &SceneDocument,
    clip_ids: &[i32],
    catalog: &EffectCatalog,
) -> (ExoFile, ExportReport) {
    let clips = document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene.id && clip_ids.contains(&clip.id))
        .collect::<Vec<_>>();
    if clip_ids
        .iter()
        .any(|id| !clips.iter().any(|clip| clip.id == *id))
    {
        // The selection can name a clip in another scene; that is not an error, but the
        // report has to say the object is not part of the file.
        let mut report_scope = Vec::new();
        for id in clip_ids {
            if !clips.iter().any(|clip| clip.id == *id) {
                report_scope.push(*id);
            }
        }
        let (file, mut report) = build_file(document, scene, clips, catalog, true);
        for id in report_scope {
            report
                .skipped
                .push(format!("clip #{id}: not in this scene"));
        }
        return (file, report);
    }
    build_file(document, scene, clips, catalog, true)
}

fn build_file(
    document: &ProjectDocument,
    scene: &SceneDocument,
    mut clips: Vec<&ClipDocument>,
    catalog: &EffectCatalog,
    selection_only: bool,
) -> (ExoFile, ExportReport) {
    let mut report = ExportReport {
        selection_only,
        ..ExportReport::default()
    };
    let mut objects = Vec::new();
    // AviUtl stacks layers upwards and objects are read in file order, so write the lowest
    // layer first and keep the original order inside a layer.
    clips.sort_by_key(|clip| (clip.layer, clip.start, clip.id));

    // Paths inside the object data use the document code page, same as the text.
    let encoding = TextEncoding::Gbk;

    for clip in clips {
        match export_clip(clip, catalog, &mut report, encoding) {
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
    encoding: TextEncoding,
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
    // A clip this build can only partly express must not be written: ExEdit would load an
    // object whose target, source, or figure data is missing and silently show something
    // else. A placeholder is the exception, because it is reproduced verbatim from the data
    // the importer kept.
    if object_effect.id != PLACEHOLDER_ID && !object_is_expressible(&clip.clip_type) {
        report.skipped.push(format!(
            "{} (layer {}, frame {}): no AviUtl object for this type",
            clip.clip_type,
            clip.layer + 1,
            clip.start + 1
        ));
        return None;
    }
    let first = if object_effect.id == PLACEHOLDER_ID {
        placeholder_filter(object_effect)?
    } else if object_mapping_for_id(&clip.clip_type).is_none() {
        // ExEdit stores 図形 as one object whose figure type lives in its own block, so the
        // shape objects have no object mapping to rebuild from.
        figure_filter(&clip.clip_type, object_effect)?
    } else {
        object_filter_from_effect(&clip.clip_type, object_effect, catalog, encoding)?
    };

    let mut filters = vec![first];
    let transform = clip.effects.iter().find(|effect| effect.id == "transform");
    // Restore the internal filters ExEdit keeps in the stack but the importer parked on the
    // clip, in their original position just before the drawing filter.
    for filter in internal_filters(clip) {
        filters.push(filter);
    }
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

    let flag = |key: &str| clip.extra.get(key).and_then(Value::as_bool) == Some(true);
    Some(ExoObject {
        start: clip.start.saturating_add(1),
        end: clip.start.saturating_add(clip.duration).max(1),
        layer: clip.layer.saturating_add(1),
        group: clip
            .extra
            .get("exoGroup")
            .and_then(Value::as_i64)
            .and_then(|group| i32::try_from(group).ok()),
        overlay: clip.clip_by_upper_object,
        clipping: flag("exoClipping"),
        // ExEdit writes `camera` on every object: `1` for the object that drives a camera,
        // `0` for everything else.
        camera: Some(clip.clip_type == "camera_control" || flag("exoCamera")),
        audio: flag("exoAudioObject"),
        chain: flag("exoChain"),
        filters,
        extra: Vec::new(),
    })
}

fn object_filter_from_effect(
    clip_type: &str,
    effect: &EffectDocument,
    _catalog: &EffectCatalog,
    encoding: TextEncoding,
) -> Option<ExoFilter> {
    let mapping = object_mapping_for_id(clip_type)?;
    let mut filter = ExoFilter::default();
    set_display_name(&mut filter, effect, mapping.aliases[0]);
    if mapping.object_id == "text" {
        if let Some(font) = effect.params.get("fontFamily").and_then(Value::as_str)
            && !font.is_empty()
        {
            filter.set("font", font);
        }
        // These three are trackbars, so an animated value has to keep its range form.
        for (parameter, key) in [
            ("fontSize", "大小"),
            ("letterSpacing", "spacing_x"),
            ("lineSpacing", "spacing_y"),
        ] {
            write_track_named(&mut filter, effect, parameter, key, 1.0);
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
        // ExEdit's `align` is a packed setting whose encoding this build has not established,
        // so an imported value is written back verbatim instead of being reconstructed from
        // our own alignment number. A text object created here has no preserved value and
        // keeps ExEdit's default.
        if let Some(align) = effect.params.get("exo_align").and_then(Value::as_str) {
            filter.set("align", align);
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
            // ExEdit's blob has a layout this build does not reproduce: it may carry flags
            // or padding around the path. When the path has not changed since the import,
            // write the original bytes back untouched; only a new or edited path is built
            // from scratch.
            let unchanged = effect
                .params
                .get("exoPath")
                .and_then(Value::as_str)
                .is_some_and(|imported| imported == path);
            match effect.params.get("exoExdata").and_then(Value::as_str) {
                Some(exdata) if unchanged => {
                    filter.set("_exdata", exdata);
                }
                _ => {
                    // ExEdit stores the referenced file as an ANSI path, not UTF-8, so a
                    // non-ASCII directory name has to use the document's code page.
                    let mut blob = vec![0u8; 4];
                    blob.extend_from_slice(&encoding.encode(path));
                    blob.push(0);
                    filter.set("_exdata", hex_encode(&blob));
                }
            }
        }
    }
    if !effect.enabled {
        filter.set("_disable", "1");
    }
    append_source_entries(&mut filter, effect);
    Some(filter)
}

/// Reports whether this clip type has an ExEdit object block this build can write in full.
///
/// Text and the media objects carry their whole state in parameters this exporter writes.
/// The figure types are rebuilt from the geometry the importer stored. Anything else — a
/// scene reference needs a target scene number, a camera control needs a target layer — would
/// be written without the field that gives it meaning.
fn object_is_expressible(clip_type: &str) -> bool {
    matches!(clip_type, "text" | "image" | "video" | "audio")
        || crate::exo_mapping::FIGURE_TYPES
            .iter()
            .any(|mapping| mapping.object_id == clip_type)
}

/// Rebuilds the ExEdit-internal filters the importer stored on the clip.
fn internal_filters(clip: &ClipDocument) -> Vec<ExoFilter> {
    let Some(filters) = clip
        .extra
        .get("exoInternalFilters")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    filters
        .iter()
        .filter_map(|filter| {
            let entries = filter.as_array()?;
            Some(ExoFilter::new(entries.iter().filter_map(|entry| {
                let pair = entry.as_array()?;
                Some((
                    pair.first()?.as_str()?.to_owned(),
                    pair.get(1)?.as_str()?.to_owned(),
                ))
            })))
        })
        .collect()
}

/// Rebuilds ExEdit's 図形 block from a translated shape object.
///
/// The inverse of the importer's conversion: the widest axis becomes `大小`, and a narrow
/// second axis becomes a `长宽比` percentage. The type index comes back from the catalogue id,
/// shape kind, and vertex count the importer wrote.
fn figure_filter(clip_type: &str, effect: &EffectDocument) -> Option<ExoFilter> {
    let sides = effect.params.get("sides").and_then(Value::as_i64)?;
    let shape_type = effect.params.get("shapeType").and_then(Value::as_str);
    let kind = crate::exo_mapping::figure_type_for(clip_type, shape_type, sides)?;
    let width = effect
        .params
        .get("sizeW")
        .and_then(Value::as_f64)
        .unwrap_or(60.0);
    let height = effect
        .params
        .get("sizeH")
        .and_then(Value::as_f64)
        .unwrap_or(width);
    let size = width.max(height) / 2.0;
    let aspect = if width >= height {
        (1.0 - height / width.max(1.0)) * 100.0
    } else {
        -(1.0 - width / height.max(1.0)) * 100.0
    };
    let mut filter = ExoFilter::default();
    set_display_name(&mut filter, effect, crate::exo_mapping::FIGURE_ALIASES[0]);
    // An animated 大小 is stored on `sizeW`, with both extents scaled from it.
    match effect
        .keyframes
        .as_ref()
        .and_then(|tracks| tracks.get("sizeW"))
        .and_then(track_end_value)
    {
        Some(end) => {
            let (motion, motion_parameter) = effect
                .keyframes
                .as_ref()
                .and_then(|tracks| tracks.get("sizeW"))
                .and_then(stored_motion)
                .unzip();
            filter.set(
                "大小",
                TrackValue {
                    start: size,
                    end: Some(end / 2.0),
                    motion,
                    motion_parameter: motion_parameter
                        .flatten()
                        .and_then(|value| i32::try_from(value).ok()),
                }
                .to_text(),
            );
        }
        None => filter.set("大小", number_text(size)),
    }
    filter.set("长宽比", number_text(round_to(aspect, 2)));
    if let Some(stroke) = effect.params.get("strokeWidth").and_then(Value::as_f64)
        && stroke > 0.0
    {
        filter.set("线宽", number_text(stroke * 100.0));
    }
    filter.set("type", kind.to_string());
    if let Some(color) = effect.params.get("color").and_then(Value::as_str) {
        filter.set("color", color.trim_start_matches('#'));
    }
    if !effect.enabled {
        filter.set("_disable", "1");
    }
    append_source_entries(&mut filter, effect);
    Some(filter)
}

fn round_to(value: f64, decimals: i32) -> f64 {
    let factor = 10f64.powi(decimals);
    (value * factor).round() / factor
}

/// Sets the display name AviUtl will look the filter up by.
///
/// AviUtl resolves `_name` against its own table, so a file written by the CN pack has to keep
/// the Chinese name and a file from stock ExEdit keeps the Japanese one. `exoName` carries the
/// exact string the source used; only an object created here falls back to the table default.
fn set_display_name(filter: &mut ExoFilter, effect: &EffectDocument, fallback: &str) {
    let name = effect
        .params
        .get("exoName")
        .and_then(Value::as_str)
        .unwrap_or(fallback);
    filter.set("_name", name);
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
    set_display_name(&mut filter, effect, DRAWING_ALIASES[0]);
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
    write_track_named(
        filter,
        effect,
        mapping.param,
        mapping.keys[0],
        mapping.scale,
    );
}

/// Writes one trackbar under an explicit key and scale.
///
/// Objects whose ExEdit parameter names are fixed rather than looked up in a mapping table
/// use this directly.
fn write_track_named(
    filter: &mut ExoFilter,
    effect: &EffectDocument,
    param: &str,
    key: &str,
    scale: f64,
) {
    let Some(value) = effect.params.get(param).and_then(Value::as_f64) else {
        return;
    };
    let raw = value / scale;
    let track = effect
        .keyframes
        .as_ref()
        .and_then(|tracks| tracks.get(param));
    match track.and_then(track_end_value) {
        Some(end) => {
            let end = end / scale;
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
    set_display_name(&mut filter, effect, mapping.aliases[0]);
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
    fn media_paths_are_written_in_the_document_code_page() {
        // ExEdit reads the referenced file with the ANSI APIs, so a path containing Chinese or
        // Japanese has to be encoded, not written as raw UTF-8.
        let session = crate::project_io::ProjectSession::blank_with(Default::default());
        let scene = session.document.scenes[0].clone();
        let mut document = session.document.clone();
        document.clips.push(ClipDocument {
            id: 1,
            scene_id: scene.id,
            clip_type: "image".to_owned(),
            start: 0,
            duration: 10,
            layer: 0,
            clip_by_upper_object: false,
            params: Default::default(),
            audio_plugins: Vec::new(),
            effects: vec![
                EffectDocument {
                    id: "transform".to_owned(),
                    name: "transform".to_owned(),
                    enabled: true,
                    params: Default::default(),
                    keyframes: None,
                    extra: Default::default(),
                },
                EffectDocument {
                    id: "image".to_owned(),
                    name: "image".to_owned(),
                    enabled: true,
                    params: serde_json::Map::from_iter([(
                        "path".to_owned(),
                        Value::from(
                            "C:\\materials\\picture.png".replace("materials", "\u{7d20}\u{6750}"),
                        ),
                    )]),
                    keyframes: None,
                    extra: Default::default(),
                },
            ],
            extra: Default::default(),
        });
        let (catalog, _) = crate::effect_catalog::EffectCatalog::load();
        let (file, _) = export_scene(&document, &scene, &catalog);
        let exdata = file.objects[0]
            .object_filter()
            .expect("object filter")
            .hex_payload("_exdata")
            .expect("the path is stored as binary data");
        let utf8 = String::from_utf8_lossy(&exdata);
        assert!(
            !utf8.contains('\u{7d20}'),
            "the path must not be stored as UTF-8: {utf8}"
        );
        // GBK for 素材, which is what ExEdit will read back.
        assert!(
            exdata
                .windows(4)
                .any(|window| window == [0xcb, 0xd8, 0xb2, 0xc4]),
            "expected GBK bytes for the directory name"
        );
    }

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
    fn clips_without_a_complete_object_block_are_reported() {
        // A scene reference needs a target scene number this exporter does not write; writing
        // the object anyway would give AviUtl an object that points nowhere.
        assert!(!object_is_expressible("scene"));
        assert!(!object_is_expressible("camera_control"));
        assert!(!object_is_expressible("counter"));
        assert!(object_is_expressible("text"));
        assert!(object_is_expressible("image"));
        assert!(object_is_expressible("audio"));
        assert!(object_is_expressible("rect"));
        assert!(object_is_expressible("polygon_shape"));
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
