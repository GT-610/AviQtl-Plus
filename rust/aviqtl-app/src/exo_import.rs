//! Imports AviUtl ExEdit object files into project clips.
//!
//! The object-file reader lives in `aviqtl_rust_core::exo` and is purely textual. This module
//! is the part that knows what the names mean: it maps ExEdit's localized object and filter
//! names onto catalogue entries, converts parameter units, turns ExEdit's start/end trackbar
//! ranges into keyframes, and preserves everything it cannot translate as a disabled
//! placeholder so that a later export can still write it back.
//!
//! Translation is deliberately conservative. A filter is only converted when both the name
//! and the parameter semantics are known (see `exo_mapping`); guessing would silently change
//! the picture, while a placeholder is visible in the effect stack and can be inspected.

use crate::effect_catalog::EffectCatalog;
use crate::exo_mapping::{
    BLEND_MODES, DRAWING_TRACKS, FilterMapping, ObjectMapping, TrackMapping, filter_mapping,
    is_drawing_filter, object_mapping,
};
use aviqtl_rust_core::api::{
    ClipDocument, EffectDocument, EffectMetadata, ProjectDocument, SceneDocument,
};
use aviqtl_rust_core::exo::{ExoFile, ExoFilter, ExoObject, TrackValue};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Catalogue id used for an object or filter this build cannot translate.
pub const PLACEHOLDER_ID: &str = "aviutl.exo.placeholder";

/// Extensions used when looking for a media path inside an object's `_exdata` blob.
const MEDIA_EXTENSIONS: &[&str] = &[
    "avi", "bmp", "flac", "gif", "jpeg", "jpg", "m4a", "mkv", "mov", "mp3", "mp4", "ogg", "png",
    "wav", "webm", "wmv",
];

/// Largest object file this importer will read. Real files are a few hundred kilobytes; the
/// cap keeps a malformed or hostile path from allocating without bound.
pub const MAX_OBJECT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Reads and parses an AviUtl object file from disk.
pub fn load_object_file(path: &std::path::Path) -> Result<ExoFile, String> {
    let bytes = crate::file_io::read_bounded(path, MAX_OBJECT_FILE_BYTES)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    ExoFile::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// Writes an object file, replacing any existing file atomically.
pub fn save_object_file(path: &std::path::Path, file: &ExoFile) -> Result<(), String> {
    crate::project_io::write_atomic(path, &file.to_bytes())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The first frame and lowest layer the file's objects occupy, in 0-based timeline units.
///
/// AviUtl places a dropped object file relative to its earliest object, and appends an
/// imported one after the current data, so both cases need to know where the file starts
/// rather than assuming frame zero.
pub fn file_anchor(file: &ExoFile) -> (i32, i32) {
    let frame = file
        .objects
        .iter()
        .map(|object| object.start.saturating_sub(1))
        .min()
        .unwrap_or(0);
    let layer = file
        .objects
        .iter()
        .map(|object| object.layer.saturating_sub(1))
        .min()
        .unwrap_or(0);
    (frame.max(0), layer.max(0))
}

/// Where the imported objects go.
#[derive(Debug, Clone, Copy)]
pub struct ImportPlacement {
    /// Added to every object's first frame. ExEdit frames are 1-based on disk.
    pub frame_offset: i32,
    /// Added to every object's layer. ExEdit layers are 1-based on disk.
    pub layer_offset: i32,
    /// Upper bound on layers in the destination scene.
    pub maximum_layers: i32,
}

impl Default for ImportPlacement {
    fn default() -> Self {
        Self {
            frame_offset: 0,
            layer_offset: 0,
            maximum_layers: 512,
        }
    }
}

/// What an import did, for the status line and the user-facing summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub objects: usize,
    /// Objects whose own filter was translated rather than preserved.
    pub translated: usize,
    /// Filters and objects preserved as disabled placeholders. This counts items, not
    /// objects: one translated object can carry several preserved filters.
    pub placeholders: usize,
    /// Human-readable notes, one per approximated or preserved item.
    pub notes: Vec<String>,
}

impl ImportReport {
    /// One-line summary suitable for the status bar.
    pub fn summary(&self) -> String {
        let mut summary = format!(
            "Imported {} object(s): {} with a translated object, {} item(s) preserved",
            self.objects, self.translated, self.placeholders
        );
        if !self.notes.is_empty() {
            summary.push_str(&format!(", {} note(s)", self.notes.len()));
        }
        summary
    }

    /// Multi-line report for a dialog, limited to `max_notes` entries.
    ///
    /// A user who imports a project and sees a different picture needs to know why, and the
    /// status bar only has room for counts. The notes name each object or filter that was
    /// preserved rather than translated.
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

/// Clips produced by an import, in file order, plus what happened to each.
#[derive(Debug, Clone, Default)]
pub struct ImportOutcome {
    pub clips: Vec<ClipDocument>,
    pub report: ImportReport,
}

/// Plans an import of `file` into `scene`.
///
/// The caller supplies the first clip id to use; ids are allocated consecutively. Nothing is
/// applied to the document here, so the workspace can run the whole import as one reversible
/// transaction.
pub fn plan_import(
    file: &ExoFile,
    scene: &SceneDocument,
    catalog: &EffectCatalog,
    first_clip_id: i32,
    placement: ImportPlacement,
) -> ImportOutcome {
    let mut outcome = ImportOutcome::default();
    let maximum_layer = placement.maximum_layers.clamp(1, 512) - 1;
    for (index, object) in file.objects.iter().enumerate() {
        let clip_id = first_clip_id.saturating_add(index as i32);
        let Some(clip) = translate_object(
            object,
            scene,
            catalog,
            clip_id,
            placement,
            maximum_layer,
            &mut outcome.report,
        ) else {
            continue;
        };
        outcome.report.objects += 1;
        outcome.clips.push(clip);
    }
    outcome
}

fn translate_object(
    object: &ExoObject,
    scene: &SceneDocument,
    catalog: &EffectCatalog,
    clip_id: i32,
    placement: ImportPlacement,
    maximum_layer: i32,
    report: &mut ImportReport,
) -> Option<ClipDocument> {
    let duration = object.duration().max(1);
    let start = object
        .start
        .saturating_sub(1)
        .saturating_add(placement.frame_offset)
        .max(0);
    let layer = object
        .layer
        .saturating_sub(1)
        .saturating_add(placement.layer_offset)
        .clamp(0, maximum_layer);
    let Some(object_filter) = object.object_filter() else {
        report.placeholders += 1;
        report
            .notes
            .push(format!("object at frame {} has no filters", object.start));
        return None;
    };

    let display = object_filter.display_name();
    let mut effects = Vec::new();
    let mut is_object_placeholder = false;
    let (clip_type, object_effect) = match figure_shape(object_filter, catalog) {
        Some((clip_type, effect)) => {
            report.translated += 1;
            (clip_type, effect)
        }
        None => match object_mapping(display) {
            Some(mapping) => match build_object_effect(object_filter, mapping, catalog) {
                Some(effect) => {
                    report.translated += 1;
                    (mapping.object_id.to_owned(), effect)
                }
                None => {
                    report.placeholders += 1;
                    report.notes.push(format!(
                        "{display}: the {} object is not available",
                        mapping.object_id
                    ));
                    (placeholder_type(display), placeholder_effect(object_filter))
                }
            },
            None => {
                report.placeholders += 1;
                report
                    .notes
                    .push(placeholder_note(display, object_filter.script_name()));
                is_object_placeholder = true;
                (placeholder_type(display), placeholder_effect(object_filter))
            }
        },
    };

    // ExEdit stores the drawing/transform filter last; everything between the object and it
    // is an effect in evaluation order.
    let filters = &object.filters;
    let drawing_index = filters
        .iter()
        .rposition(|filter| is_drawing_filter(filter.display_name()));
    let effect_end = drawing_index.unwrap_or(filters.len());
    // ExEdit appends these itself; they carry no artist intent and translating them would add
    // a filter AviUtl never showed in the object's stack.
    let internal = |filter: &ExoFilter| {
        matches!(
            filter.display_name(),
            "上层物件缓存" | "上層オブジェクトバッファ" | "直前オブジェクト" | "フレームバッファ"
        )
    };
    let mut preserved = Vec::new();
    for filter in filters.iter().take(effect_end).skip(1) {
        if internal(filter) {
            preserved.push(filter.clone());
            continue;
        }
        effects.push(translate_filter(filter, catalog, report));
    }

    let transform_source = drawing_index.and_then(|index| filters.get(index));
    // An object without a drawing filter has no transform in AviUtl either; adding one here
    // would change the picture and add a filter the source file never had.
    let mut transform = transform_source.and_then(|filter| build_transform(filter, catalog));
    if let Some(filter) = transform_source
        && filter.is_disabled()
        && let Some(effect) = transform.as_mut()
    {
        effect.enabled = false;
    }
    if let Some(transform) = transform {
        effects.insert(0, transform);
    }
    let mut object_effect = object_effect;
    if is_object_placeholder {
        // ExEdit's other filters can also be placeholders, so the object's own placeholder
        // carries a marker the exporter can find.
        object_effect
            .params
            .insert("exoObject".to_owned(), Value::Bool(true));
    }
    effects.push(object_effect);

    Some(ClipDocument {
        id: clip_id,
        scene_id: scene.id,
        clip_type,
        start,
        duration,
        layer,
        clip_by_upper_object: object.overlay,
        params: Map::new(),
        audio_plugins: Vec::new(),
        effects,
        // Kept on the clip rather than in the stack: AviUtl's own filter list is the only
        // place these belong, and the exporter writes them back unchanged.
        extra: preserved_clip_extras(object, &preserved),
    })
}

/// Stores the ExEdit-only object state a clip has no field for, so an export can
/// reproduce it.
///
/// `group`, `clipping`, `audio` and `chain` are placement and routing flags ExEdit writes on
/// the object section; AviQtl-Plus has no equivalent, but dropping them would change which
/// object an effect applies to after a round trip. `camera` is kept unless it is the plain
/// `0` every ordinary object carries.
fn preserved_clip_extras(object: &ExoObject, filters: &[ExoFilter]) -> BTreeMap<String, Value> {
    let mut extras = BTreeMap::new();
    if let Some(group) = object.group {
        extras.insert("exoGroup".to_owned(), Value::from(group));
    }
    if object.clipping {
        extras.insert("exoClipping".to_owned(), Value::Bool(true));
    }
    if object.audio {
        extras.insert("exoAudioObject".to_owned(), Value::Bool(true));
    }
    if object.chain {
        extras.insert("exoChain".to_owned(), Value::Bool(true));
    }
    if object.camera == Some(true) {
        extras.insert("exoCamera".to_owned(), Value::Bool(true));
    }
    if filters.is_empty() {
        return extras;
    }
    let entries = filters
        .iter()
        .map(|filter| {
            Value::Array(
                filter
                    .entries
                    .iter()
                    .map(|(key, value)| json!([key, value]))
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    extras.insert("exoInternalFilters".to_owned(), Value::Array(entries));
    extras
}

/// Translates ExEdit's 図形 object, which is a solid regular polygon.
///
/// `FUN_10073470` hands the object's size to the rasteriser as the polygon's half-extent:
/// the vertex loops run from `-size` to `+size`, so a figure is `2 * 大小` across. A positive
/// `长宽比` shrinks the vertical axis by that percentage and a negative one shrinks the
/// horizontal axis, which maps onto our width and height.
fn figure_shape(filter: &ExoFilter, catalog: &EffectCatalog) -> Option<(String, EffectDocument)> {
    if !crate::exo_mapping::FIGURE_ALIASES.contains(&filter.display_name()) {
        return None;
    }
    let kind = filter.named_integer("type").unwrap_or(1);
    let mapping = crate::exo_mapping::figure_mapping(kind);
    let metadata = catalog.find(mapping.object_id)?;
    let mut effect = effect_from_metadata(metadata);
    let size_track = filter
        .named_track("大小")
        .or_else(|| filter.track(0))
        .unwrap_or_else(|| TrackValue::constant(30.0));
    let size = size_track.start;
    let aspect = filter
        .named_track("长宽比")
        .or_else(|| filter.track(1))
        .map_or(0.0, |track| track.start);
    let (width, height) = if aspect >= 0.0 {
        (size * 2.0, size * 2.0 * (1.0 - aspect / 100.0).max(0.0))
    } else {
        (size * 2.0 * (1.0 + aspect / 100.0).max(0.0), size * 2.0)
    };
    effect.params.insert("sizeW".to_owned(), number(width));
    effect.params.insert("sizeH".to_owned(), number(height));
    effect
        .params
        .insert("sides".to_owned(), Value::from(mapping.sides));
    if let Some(shape_type) = mapping.shape_type {
        effect
            .params
            .insert("shapeType".to_owned(), Value::from(shape_type));
    }
    if let Some(color) = filter.color("color") {
        effect
            .params
            .insert("color".to_owned(), Value::from(format!("#{color:06x}")));
    }
    // `線幅` is stored at the same hundredths scale as the other figure tracks.
    if let Some(line_width) = filter
        .named_track("线宽")
        .or_else(|| filter.track(2))
        .map(|track| track.start / 100.0)
        && line_width > 0.0
    {
        effect
            .params
            .insert("strokeWidth".to_owned(), number(line_width));
        if let Some(color) = filter.color("color") {
            effect.params.insert(
                "strokeColor".to_owned(),
                Value::from(format!("#{color:06x}")),
            );
        }
    }
    // An animated 大小 drives both extents, so it becomes a keyframe track on the width and
    // the export writes the range back.
    if let Some(end) = size_track.end
        && (end - size).abs() > f64::EPSILON
    {
        let interpolation = size_track
            .motion
            .as_deref()
            .map(interpolation_for)
            .unwrap_or("linear");
        let mut options = json!({"interp": interpolation});
        if let Some(motion) = &size_track.motion
            && let Some(options) = options.as_object_mut()
        {
            options.insert("exoMotion".to_owned(), Value::from(motion.clone()));
            if let Some(parameter) = size_track.motion_parameter {
                options.insert("exoMotionParameter".to_owned(), Value::from(parameter));
            }
        }
        let keyframes = effect.keyframes.get_or_insert_with(Map::new);
        keyframes.insert(
            "sizeW".to_owned(),
            json!({
                "start": {"frame": 0, "value": number(size * 2.0), "interp": interpolation},
                "points": [{"frame": 0, "value": number(end * 2.0), "interp": interpolation, "options": options}],
            }),
        );
    }
    capture_source_entries(&mut effect, filter);
    effect.enabled = !filter.is_disabled();
    apply_metadata_defaults(&mut effect, metadata);
    Some((mapping.object_id.to_owned(), effect))
}

/// A clip type that no renderer claims, so an untranslated object draws nothing and the
/// effect stack still explains what it was.
fn placeholder_type(display: &str) -> String {
    format!("exo.{display}")
}

fn catalog_effect(catalog: &EffectCatalog, id: &str) -> Option<EffectDocument> {
    // `effect_document` only serves effects, so objects are built from their metadata here.
    catalog.find(id).map(effect_from_metadata)
}

/// Builds an effect document from catalogue metadata, the same way new objects start life.
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

fn build_object_effect(
    filter: &ExoFilter,
    mapping: &ObjectMapping,
    catalog: &EffectCatalog,
) -> Option<EffectDocument> {
    let metadata = catalog.find(mapping.object_id)?;
    let mut effect = catalog_effect(catalog, mapping.object_id)?;
    apply_mappings(
        &mut effect,
        filter,
        mapping.tracks,
        mapping.colors,
        mapping.integers,
        mapping.flags,
    );
    capture_source_entries(&mut effect, filter);
    effect.enabled = !filter.is_disabled();
    if mapping.object_id == "text" {
        if let Some(text) = filter.utf16_text("text") {
            effect.params.insert("text".to_owned(), Value::from(text));
        }
        if let Some(font) = filter.value("font").filter(|font| !font.is_empty()) {
            effect
                .params
                .insert("fontFamily".to_owned(), Value::from(font.to_owned()));
        }
        apply_text_alignment(&mut effect, filter);
        // AviUtl always paints the outline colour behind the glyph fill, so an imported text
        // object keeps its outline unless the file omits the colour entirely.
        if filter.color("color2").is_some() {
            effect
                .params
                .insert("outlineEnabled".to_owned(), Value::Bool(true));
            if !effect.params.contains_key("outlineWidth") {
                effect
                    .params
                    .insert("outlineWidth".to_owned(), Value::from(2.0));
            }
        }
    }
    if mapping.media_path {
        match filter
            .hex_payload("_exdata")
            .and_then(|blob| extract_media_path(&blob))
        {
            Some(path) => {
                let key = if mapping.object_id == "audio" {
                    "source"
                } else {
                    "path"
                };
                // Remember the path as imported. ExEdit's binary blob has a layout this
                // build does not reproduce, so an unchanged path is written back inside the
                // original bytes instead of a reconstructed blob that may drop fields.
                effect
                    .params
                    .insert("exoPath".to_owned(), Value::from(path.clone()));
                effect.params.insert(key.to_owned(), Value::from(path));
            }
            None => {
                return Some(effect);
            }
        }
    }
    apply_metadata_defaults(&mut effect, metadata);
    Some(effect)
}

fn build_transform(filter: &ExoFilter, catalog: &EffectCatalog) -> Option<EffectDocument> {
    let metadata = catalog.find("transform")?;
    let mut effect = catalog_effect(catalog, "transform")?;
    for mapping in DRAWING_TRACKS {
        apply_track(&mut effect, filter, mapping);
    }
    capture_source_entries(&mut effect, filter);
    if let Some(blend) = filter
        .named_integer("blend")
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| BLEND_MODES.get(index))
    {
        effect
            .params
            .insert("blendMode".to_owned(), Value::from(*blend));
    }
    apply_metadata_defaults(&mut effect, metadata);
    Some(effect)
}

fn translate_filter(
    filter: &ExoFilter,
    catalog: &EffectCatalog,
    report: &mut ImportReport,
) -> EffectDocument {
    let display = filter.display_name();
    let Some(mapping): Option<&FilterMapping> = filter_mapping(display) else {
        report.placeholders += 1;
        report
            .notes
            .push(placeholder_note(display, filter.script_name()));
        return placeholder_effect(filter);
    };
    let Some(metadata) = catalog.find(mapping.effect_id) else {
        report.placeholders += 1;
        report.notes.push(format!(
            "{display}: the {} effect is not available",
            mapping.effect_id
        ));
        return placeholder_effect(filter);
    };
    let Some(mut effect) = catalog_effect(catalog, mapping.effect_id) else {
        report.placeholders += 1;
        return placeholder_effect(filter);
    };
    apply_mappings(
        &mut effect,
        filter,
        mapping.tracks,
        mapping.colors,
        mapping.integers,
        mapping.flags,
    );
    capture_source_entries(&mut effect, filter);
    effect.enabled = !filter.is_disabled();
    apply_metadata_defaults(&mut effect, metadata);
    report.translated += 1;
    effect
}

/// Explains why something was preserved, naming the script when there is one.
///
/// Most preserved entries are script-backed effects. Naming the script turns an opaque
/// "placeholder" into something the user can act on: the script is a file in AviUtl's
/// `script` folder, and knowing which one is missing is the difference between a usable note
/// and a shrug.
fn placeholder_note(display: &str, script: Option<&str>) -> String {
    match script.filter(|script| !script.is_empty()) {
        Some(script) => format!(
            "{display}: needs the ExEdit script {script}, preserved as a disabled placeholder"
        ),
        None => {
            format!("{display}: no AviQtl-Plus equivalent, preserved as a disabled placeholder")
        }
    }
}

/// Preserves an object or filter this build cannot translate.
fn placeholder_effect(filter: &ExoFilter) -> EffectDocument {
    let display = filter.display_name();
    let mut params = Map::new();
    params.insert("exoDisplayName".to_owned(), Value::from(display));
    if let Some(script) = filter.script_name() {
        params.insert("exoScript".to_owned(), Value::from(script));
    }
    if let Some(parameter) = filter.script_parameter() {
        params.insert("exoScriptParameter".to_owned(), Value::from(parameter));
    }
    let tracks = filter
        .entries
        .iter()
        .filter(|(key, _)| key.starts_with("track"))
        .map(|(key, value)| (key.clone(), Value::from(value.clone())))
        .collect::<Map<_, _>>();
    if !tracks.is_empty() {
        params.insert("exoTracks".to_owned(), Value::Object(tracks));
    }
    let checks = filter
        .entries
        .iter()
        .filter(|(key, _)| key.starts_with("check"))
        .map(|(key, value)| (key.clone(), Value::from(value.clone())))
        .collect::<Map<_, _>>();
    if !checks.is_empty() {
        params.insert("exoChecks".to_owned(), Value::Object(checks));
    }
    if let Some(exdata) = filter.hex_payload("_exdata") {
        params.insert("exoExdata".to_owned(), Value::from(hex_encode(&exdata)));
    }
    // The placeholder itself is always disabled so it never changes the picture, but the
    // file's own disabled state has to survive an export separately from that.
    if filter.is_disabled() {
        params.insert("exoDisabled".to_owned(), Value::Bool(true));
    }
    for (key, value) in &filter.entries {
        // `name` and `param` are also kept verbatim: script_name and script_parameter
        // above only store their non-empty forms, and an empty `name=` still has to come
        // back out.
        let is_structurally_known =
            key.starts_with("track") || matches!(key.as_str(), "_name" | "_exdata" | "_disable");
        if !is_structurally_known {
            params.insert(format!("exo_{key}"), Value::from(value.clone()));
        }
    }
    EffectDocument {
        id: PLACEHOLDER_ID.to_owned(),
        name: if display.is_empty() {
            "AviUtl object".to_owned()
        } else {
            display.to_owned()
        },
        enabled: false,
        params,
        keyframes: None,
        extra: BTreeMap::new(),
    }
}

/// Keeps every source key on a translated effect so exporting can reproduce the file.
///
/// ExEdit filters carry keys this build does not interpret - positional `type`/`filter`
/// markers, script parameters, text-object toggles such as `autoadjust`, and the original
/// disabled flag. They are stored verbatim under an `exo_` prefix and written back only when
/// the exporter has not already produced the same key, which makes a round trip lossless
/// without teaching the mapper about each one.
fn capture_source_entries(effect: &mut EffectDocument, filter: &ExoFilter) {
    // The localized display name decides how AviUtl finds the filter, and the CN pack,
    // stock ExEdit, and other translations each spell it differently. Keep the exact string
    // the file used so an export is readable by the same build it came from.
    let display = filter.display_name();
    if !display.is_empty() {
        effect
            .params
            .insert("exoName".to_owned(), Value::from(display.to_owned()));
    }
    for (key, value) in &filter.entries {
        if matches!(key.as_str(), "_name" | "_disable" | "_exdata") {
            continue;
        }
        effect
            .params
            .insert(format!("exo_{key}"), Value::from(value.clone()));
    }
    if let Some(exdata) = filter.hex_payload("_exdata") {
        effect
            .params
            .insert("exoExdata".to_owned(), Value::from(hex_encode(&exdata)));
    }
}

fn apply_mappings(
    effect: &mut EffectDocument,
    filter: &ExoFilter,
    tracks: &[TrackMapping],
    colors: &[crate::exo_mapping::ColorMapping],
    integers: &[crate::exo_mapping::IntegerMapping],
    flags: &[(&[&str], &str)],
) {
    for mapping in tracks {
        apply_track(effect, filter, mapping);
    }
    for mapping in colors {
        if let Some(color) = mapping.keys.iter().find_map(|key| filter.color(key)) {
            effect.params.insert(
                mapping.param.to_owned(),
                Value::from(format!("#{color:06x}")),
            );
        }
    }
    for mapping in integers {
        if let Some(value) = mapping
            .keys
            .iter()
            .find_map(|key| filter.named_integer(key))
        {
            let converted = f64::from(value) * mapping.scale + mapping.offset;
            effect
                .params
                .insert(mapping.param.to_owned(), number(converted));
        }
    }
    for (keys, param) in flags {
        if let Some(value) = keys.iter().find_map(|key| filter.named_flag(key)) {
            effect
                .params
                .insert((*param).to_owned(), Value::Bool(value));
        }
    }
}

/// Converts one trackbar, writing a keyframe track when the value changes across the object.
fn apply_track(effect: &mut EffectDocument, filter: &ExoFilter, mapping: &TrackMapping) {
    let Some(track) = mapping
        .keys
        .iter()
        .find_map(|key| filter.named_track(key).or_else(|| track_at(filter, key)))
    else {
        return;
    };
    let start = track.start * mapping.scale + mapping.offset;
    effect
        .params
        .insert(mapping.param.to_owned(), number(start));
    let Some(end) = track.end else {
        return;
    };
    let end = end * mapping.scale + mapping.offset;
    if (end - start).abs() <= f64::EPSILON {
        return;
    }
    let interpolation = track
        .motion
        .as_deref()
        .map(interpolation_for)
        .unwrap_or("linear");
    let mut options = json!({"interp": interpolation});
    if let Some(motion) = &track.motion
        && let Some(options) = options.as_object_mut()
    {
        options.insert("exoMotion".to_owned(), Value::from(motion.clone()));
        if let Some(parameter) = track.motion_parameter {
            options.insert("exoMotionParameter".to_owned(), Value::from(parameter));
        }
    }
    let keyframes = effect.keyframes.get_or_insert_with(Map::new);
    keyframes.insert(
        mapping.param.to_owned(),
        json!({
            "start": {"frame": 0, "value": number(start), "interp": interpolation},
            "points": [{"frame": 0, "value": number(end), "interp": interpolation, "options": options}],
        }),
    );
}

/// Reads a positional `trackN` key, which is how script-backed filters store parameters.
fn track_at(filter: &ExoFilter, key: &str) -> Option<TrackValue> {
    let index = key.strip_prefix("track")?;
    let index: usize = index.parse().ok()?;
    filter.track(index)
}

/// Maps an ExEdit motion identifier onto one of our interpolation names.
///
/// ExEdit stores a motion as `<index>@<name>@<script>`, where the name is a label inside the
/// easing script rather than a curve identifier. Only labels whose meaning matches a built-in
/// curve are converted; everything else stays linear and keeps the original text in the
/// keyframe options so an export can write it back.
fn interpolation_for(motion: &str) -> &'static str {
    let name = motion
        .split('@')
        .nth(1)
        .unwrap_or(motion)
        .to_ascii_lowercase();
    if name.contains("普通") || name.contains("linear") {
        "linear"
    } else if name.contains("加速") && name.contains("減速") || name.contains("加減速") {
        "ease_in_out_cubic"
    } else if name.contains("加速") {
        "ease_in_cubic"
    } else if name.contains("減速") || name.contains("减速") {
        "ease_out_cubic"
    } else {
        "linear"
    }
}

/// ExEdit's `align` is an index into its 揃え list; ours is a bit flag.
fn apply_text_alignment(effect: &mut EffectDocument, filter: &ExoFilter) {
    let Some(align) = filter.named_integer("align") else {
        return;
    };
    // ExEdit stores alignment as a flag: 1 left, 2 centre, 4 right, 8 justify. Our
    // `alignment` parameter reuses different numbers for the same four choices.
    let alignment = if align & 4 != 0 {
        2
    } else if align & 2 != 0 {
        4
    } else if align & 8 != 0 {
        8
    } else {
        1
    };
    effect
        .params
        .insert("alignment".to_owned(), Value::from(alignment));
}

/// Fills in parameters the file did not carry from the catalogue's own defaults.
fn apply_metadata_defaults(effect: &mut EffectDocument, metadata: &EffectMetadata) {
    for (key, value) in &metadata.params {
        effect
            .params
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
}

fn number(value: f64) -> Value {
    let value = if value.is_finite() { value } else { 0.0 };
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or_else(|| Value::from(0))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// Finds a media path inside an object's `_exdata` blob.
///
/// ExEdit stores per-object binary data whose layout depends on the object type; for media
/// objects the referenced file name is one of the NUL-terminated strings inside it. Rather
/// than hard-coding an offset that differs per ExEdit build, this scans the blob for
/// NUL-terminated runs that end in a known media extension and keeps the longest one.
pub fn extract_media_path(blob: &[u8]) -> Option<String> {
    let mut best: Option<&[u8]> = None;
    for segment in blob.split(|byte| *byte == 0) {
        let segment = trim_control(segment);
        if segment.len() < 4 || segment.len() > 1024 {
            continue;
        }
        let Some(extension) = segment.rsplit(|byte| *byte == b'.').next() else {
            continue;
        };
        let extension = String::from_utf8_lossy(extension).to_ascii_lowercase();
        if !MEDIA_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        if best.is_none_or(|current| segment.len() > current.len()) {
            best = Some(segment);
        }
    }
    let segment = best?;
    let decoded = aviqtl_rust_core::exo::TextEncoding::detect(segment);
    Some(decoded.decode(segment))
}

fn trim_control(mut segment: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = segment {
        if first.is_ascii_control() {
            segment = rest;
        } else {
            break;
        }
    }
    segment
}

/// Returns the objects of `file` that would be placed outside the destination scene.
pub fn out_of_range_objects(file: &ExoFile, document: &ProjectDocument, scene_id: i32) -> usize {
    let Some(scene) = document.scenes.iter().find(|scene| scene.id == scene_id) else {
        return 0;
    };
    let duration = scene.start.saturating_add(scene.duration);
    file.objects
        .iter()
        .filter(|object| {
            let start = object.start.saturating_sub(1);
            start >= duration || object.end < 1
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exo_export::export_scene;
    use crate::project_io::{ProjectDefaults, ProjectSession};

    #[test]
    fn finds_a_media_path_inside_an_exdata_blob() {
        let mut blob = vec![0u8, 0, 0, 0];
        blob.extend_from_slice(b"C:\\media\\clip.mp4");
        blob.push(0);
        blob.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            extract_media_path(&blob).as_deref(),
            Some("C:\\media\\clip.mp4")
        );
        assert_eq!(extract_media_path(&[0, 1, 2]), None);
        assert_eq!(extract_media_path(b"no-extension"), None);
    }

    #[test]
    fn prefers_the_longest_media_path() {
        let mut blob = Vec::new();
        blob.extend_from_slice(b"a.png");
        blob.push(0);
        blob.extend_from_slice(b"C:\\media\\long-name.mkv");
        blob.push(0);
        assert_eq!(
            extract_media_path(&blob).as_deref(),
            Some("C:\\media\\long-name.mkv")
        );
    }

    #[test]
    fn maps_exedit_motion_labels_onto_our_curves() {
        assert_eq!(interpolation_for("15@easing_普通@uf_easing"), "linear");
        assert_eq!(interpolation_for("2@加減速@builtin"), "ease_in_out_cubic");
        assert_eq!(interpolation_for("3@加速@builtin"), "ease_in_cubic");
        assert_eq!(interpolation_for("4@減速@builtin"), "ease_out_cubic");
        assert_eq!(interpolation_for("7@不明@builtin"), "linear");
    }

    /// A text object with a mapped effect and a script object, as the Chinese pack writes them.
    fn sample() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"[exedit]\r\nwidth=1280\r\nheight=720\r\nrate=30\r\nscale=100\r\nlength=120\r\n\
              audio_rate=48000\r\naudio_ch=2\r\n",
        );
        bytes.extend_from_slice(b"[0]\r\nstart=11\r\nend=40\r\nlayer=3\r\noverlay=1\r\n");
        bytes.extend_from_slice(b"[0.0]\r\n_name=");
        bytes.extend_from_slice(&[0xce, 0xc4, 0xb1, 0xbe]); // 文本
        bytes.extend_from_slice(b"\r\n\xb4\xf3\xd0\xa1=36\r\n"); // 大小
        bytes.extend_from_slice(b"color=ff8000\r\ncolor2=000000\r\nalign=1\r\nB=1\r\n");
        bytes.extend_from_slice(b"font=");
        bytes.extend_from_slice(&[0xba, 0xda, 0xcc, 0xe5]); // 黑体
        bytes.extend_from_slice(b"\r\ntext=604f7d59\r\n"); // 你好 in UTF-16LE
        bytes.extend_from_slice(b"[0.1]\r\n_name=");
        bytes.extend_from_slice(&[0xc4, 0xa3, 0xba, 0xfd]); // 模糊
        bytes.extend_from_slice(b"\r\n\xb7\xb6\xce\xa7=8\r\n"); // 范围
        bytes.extend_from_slice(b"[0.2]\r\n_name=");
        bytes.extend_from_slice(&[0xb1, 0xea, 0xd7, 0xbc, 0xca, 0xf4, 0xd0, 0xd4]); // 标准属性
        bytes.extend_from_slice(
            b"\r\nX=100.0,300.0,15@easing_\xc6\xd5\xcd\xa8@uf_easing,16\r\nY=-50.0\r\nZ=0.0\r\n\
              \xcb\xf5\xb7\xc5\xc2\xca=150.00\r\n\xcd\xb8\xc3\xf7\xb6\xc8=20.0\r\n\
              \xd0\xfd\xd7\xaa=0.00\r\nblend=2\r\n",
        );
        bytes.extend_from_slice(b"[1]\r\nstart=1\r\nend=30\r\nlayer=1\r\n[1.0]\r\n_name=");
        bytes.extend_from_slice(&[0xd7, 0xd4, 0xb6, 0xa8, 0xd2, 0xe5, 0xce, 0xef, 0xbc, 0xfe]);
        bytes.extend_from_slice(
            b"\r\nname=uf_easing\r\nparam=local P_VIEW=1;\r\ntrack0=0.00\r\ncheck0=1\r\n",
        );
        bytes
    }

    fn scene() -> SceneDocument {
        let session = ProjectSession::blank_with(ProjectDefaults::default());
        session.document.scenes[0].clone()
    }

    #[test]
    fn translates_text_transform_and_mapped_effects() {
        let (catalog, _) = EffectCatalog::load();
        let file = ExoFile::parse(&sample()).expect("sample object file");
        let outcome = plan_import(&file, &scene(), &catalog, 10, ImportPlacement::default());
        assert_eq!(outcome.clips.len(), 2);
        assert_eq!(outcome.report.objects, 2);

        let text = &outcome.clips[0];
        assert_eq!(text.id, 10);
        assert_eq!(text.clip_type, "text");
        assert_eq!((text.start, text.duration, text.layer), (10, 30, 2));
        assert!(text.clip_by_upper_object);
        assert_eq!(text.effects[0].id, "transform");
        assert_eq!(text.effects[1].id, "blur");
        assert_eq!(text.effects[1].params["size"], Value::from(8.0));
        assert_eq!(text.effects[2].id, "text");

        let body = &text.effects[2].params;
        assert_eq!(body["text"], Value::from("你好"));
        assert_eq!(body["fontFamily"], Value::from("黑体"));
        assert_eq!(body["fontSize"], Value::from(36.0));
        assert_eq!(body["color"], Value::from("#ff8000"));
        assert_eq!(body["outlineColor"], Value::from("#000000"));
        assert_eq!(body["outlineEnabled"], Value::Bool(true));
        assert_eq!(body["fontBold"], Value::Bool(true));
        // ExEdit stores 1 for left alignment on this fixture.
        assert_eq!(body["alignment"], Value::from(1));

        let transform = &text.effects[0].params;
        assert_eq!(transform["y"], Value::from(-50.0));
        assert_eq!(transform["scale"], Value::from(150.0));
        assert_eq!(transform["opacity"], Value::from(0.2));
        assert_eq!(transform["blendMode"], Value::from("乗算"));

        // The trackbar range becomes a keyframe track whose end value is 300.
        let track = &text.effects[0].keyframes.as_ref().expect("keyframes")["x"];
        assert_eq!(track["start"]["value"], Value::from(100.0));
        assert_eq!(track["points"][0]["value"], Value::from(300.0));
        assert_eq!(
            track["points"][0]["options"]["exoMotionParameter"],
            Value::from(16)
        );
    }

    #[test]
    fn translates_the_figure_object_into_a_regular_polygon() {
        let (catalog, _) = EffectCatalog::load();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"[exedit]\r\nwidth=1280\r\nheight=720\r\nrate=30\r\nscale=100\r\nlength=120\r\n\
              audio_rate=48000\r\naudio_ch=2\r\n[0]\r\nstart=1\r\nend=30\r\nlayer=1\r\n[0.0]\r\n_name=",
        );
        bytes.extend_from_slice(&[0xcd, 0xbc, 0xd0, 0xce]); // 图形
        bytes.extend_from_slice(b"\r\n\xb4\xf3\xd0\xa1=30\r\n"); // 大小 = 30
        bytes.extend_from_slice(b"\xb3\xa4\xbf\xed\xb1\xc8=50.0\r\n"); // 长宽比 = 50
        bytes.extend_from_slice(b"\xcf\xdf\xbf\xed=4000\r\n"); // 线宽 = 4000
        bytes.extend_from_slice(b"type=4\r\ncolor=ffffff\r\nname=\r\n");
        let file = ExoFile::parse(&bytes).expect("figure object file");
        let outcome = plan_import(&file, &scene(), &catalog, 1, ImportPlacement::default());
        assert_eq!(outcome.clips.len(), 1);
        let clip = &outcome.clips[0];
        // Type 4 is the pentagon, and a figure is two half-extents across.
        assert_eq!(clip.clip_type, "polygon_shape");
        let body = &clip.effects[0].params;
        assert_eq!(body["sides"], Value::from(5));
        assert_eq!(body["sizeW"], Value::from(60.0));
        assert_eq!(body["sizeH"], Value::from(30.0));
        assert_eq!(body["strokeWidth"], Value::from(40.0));
        assert_eq!(body["color"], Value::from("#ffffff"));
        assert_eq!(outcome.report.placeholders, 0);

        // Exporting reproduces the figure block the file started with.
        let session = ProjectSession::blank_with(ProjectDefaults::default());
        let mut document = session.document.clone();
        document.clips = outcome.clips.clone();
        let (exported, _) = export_scene(&document, &scene(), &catalog);
        let figure = exported.objects[0].object_filter().expect("figure filter");
        assert_eq!(figure.display_name(), "图形");
        assert_eq!(figure.value("type"), Some("4"));
        assert_eq!(figure.named_track("大小").unwrap().start, 30.0);
        assert_eq!(figure.named_track("长宽比").unwrap().start, 50.0);
        assert_eq!(figure.named_track("线宽").unwrap().start, 4000.0);
    }

    #[test]
    fn every_built_in_figure_type_round_trips() {
        let (catalog, _) = EffectCatalog::load();
        for kind in 1..=6 {
            let mapping = crate::exo_mapping::figure_mapping(kind);
            let restored = crate::exo_mapping::figure_type_for(
                mapping.object_id,
                mapping.shape_type,
                i64::from(mapping.sides),
            );
            assert_eq!(restored, Some(kind), "figure type {kind}");
            assert!(catalog.find(mapping.object_id).is_some());
        }
    }

    #[test]
    fn report_details_list_notes_and_cap_the_rest() {
        let report = ImportReport {
            objects: 3,
            translated: 2,
            placeholders: 1,
            notes: vec![
                "动画效果: preserved".to_owned(),
                "自定义物件: preserved".to_owned(),
                "图形: preserved".to_owned(),
            ],
        };
        let details = report.details(2);
        assert!(details.starts_with(&report.summary()));
        assert!(details.contains("动画效果"));
        assert!(details.contains("自定义物件"));
        assert!(!details.contains("图形: preserved"));
        assert!(details.contains("… and 1 more"));

        let clean = ImportReport {
            objects: 1,
            translated: 1,
            ..ImportReport::default()
        };
        assert_eq!(clean.details(5), clean.summary());
    }

    #[test]
    fn an_imported_filter_keeps_the_name_its_own_build_uses() {
        let (catalog, _) = EffectCatalog::load();
        // 標準描画 is what stock ExEdit writes; the CN pack writes 标准属性. Both resolve to
        // the same transform, and the exported file has to keep whichever came in.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"[exedit]\r\nwidth=1280\r\nheight=720\r\nrate=30\r\nscale=100\r\nlength=60\r\n\
              audio_rate=48000\r\naudio_ch=2\r\n[0]\r\nstart=1\r\nend=10\r\nlayer=1\r\n[0.0]\r\n_name=",
        );
        bytes.extend_from_slice(&[0xce, 0xc4, 0xb1, 0xbe]); // 文本
        bytes.extend_from_slice(b"\r\ntext=4100\r\n[0.1]\r\n_name=");
        // 標準描画 in GBK
        bytes.extend_from_slice(&[0x98, 0xcb, 0x9c, 0xca, 0xc3, 0xe8, 0xbb, 0xad]);
        bytes.extend_from_slice(b"\r\nX=10.0\r\nY=20.0\r\n");
        let file = ExoFile::parse(&bytes).expect("file");
        let outcome = plan_import(&file, &scene(), &catalog, 1, ImportPlacement::default());

        let session = ProjectSession::blank_with(ProjectDefaults::default());
        let mut document = session.document.clone();
        document.clips = outcome.clips.clone();
        let (exported, _) = export_scene(&document, &scene(), &catalog);
        let drawing = exported.objects[0]
            .drawing_filter()
            .expect("the drawing filter is kept");
        assert_eq!(
            drawing.display_name(),
            "標準描画",
            "the original spelling has to survive"
        );
    }

    #[test]
    fn a_preserved_script_names_the_file_it_needs() {
        let (catalog, _) = EffectCatalog::load();
        let file = ExoFile::parse(&sample()).expect("sample object file");
        let outcome = plan_import(&file, &scene(), &catalog, 10, ImportPlacement::default());
        let note = outcome
            .report
            .notes
            .iter()
            .find(|note| note.contains("自定义物件"))
            .expect("the script object is noted");
        assert!(
            note.contains("uf_easing"),
            "the note has to name the script: {note}"
        );
        // A filter with no script identifier keeps the plain wording.
        let plain = placeholder_note("图形", None);
        assert!(plain.contains("no AviQtl-Plus equivalent"));
        assert!(!plain.contains("needs the ExEdit script"));
    }

    #[test]
    fn an_unchanged_media_path_keeps_the_original_exdata() {
        let (catalog, _) = EffectCatalog::load();
        // A media object whose blob carries a flag before the path, as ExEdit lays it out.
        let blob: [u8; 12] = [1, 0, 0, 0, b'C', b':', b'\\', b'a', b'.', b'p', b'n', b'g'];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"[exedit]\r\nwidth=1280\r\nheight=720\r\nrate=30\r\nscale=100\r\nlength=60\r\n\
              audio_rate=48000\r\naudio_ch=2\r\n[0]\r\nstart=1\r\nend=10\r\nlayer=1\r\n[0.0]\r\n_name=",
        );
        bytes.extend_from_slice(&[0xcd, 0xbc, 0xc6, 0xac, 0xce, 0xc4, 0xbc, 0xfe]); // 图片文件
        bytes.extend_from_slice(b"\r\n_exdata=");
        for byte in blob {
            bytes.extend_from_slice(format!("{byte:02x}").as_bytes());
        }
        bytes.extend_from_slice(b"\r\n[0.1]\r\n_name=");
        bytes.extend_from_slice(&[0xb1, 0xea, 0xd7, 0xbc, 0xca, 0xf4, 0xd0, 0xd4]); // 标准属性
        bytes.extend_from_slice(b"\r\nX=0.0\r\n");

        let file = ExoFile::parse(&bytes).expect("media object file");
        let outcome = plan_import(&file, &scene(), &catalog, 1, ImportPlacement::default());
        assert_eq!(outcome.clips.len(), 1);
        let image = outcome.clips[0]
            .effects
            .iter()
            .find(|effect| effect.id == "image")
            .expect("the image object translated");
        assert_eq!(image.params["path"], Value::from("C:\\a.png"));

        let session = ProjectSession::blank_with(ProjectDefaults::default());
        let mut document = session.document.clone();
        document.clips = outcome.clips.clone();
        let (exported, _) = export_scene(&document, &scene(), &catalog);
        let written = exported.objects[0]
            .object_filter()
            .and_then(|filter| filter.hex_payload("_exdata"))
            .expect("exdata is written");
        assert_eq!(
            written,
            blob.to_vec(),
            "the original blob is kept byte for byte when the path is unchanged"
        );
    }

    #[test]
    fn preserves_script_objects_as_disabled_placeholders() {
        let (catalog, _) = EffectCatalog::load();
        let file = ExoFile::parse(&sample()).expect("sample object file");
        let outcome = plan_import(&file, &scene(), &catalog, 10, ImportPlacement::default());
        let script = &outcome.clips[1];
        assert_eq!(script.clip_type, "exo.自定义物件");
        // The script object has no drawing filter, so it stays a two-effect clip: only the
        // object placeholder, with no transform invented for it.
        assert_eq!(script.effects.len(), 1);
        let placeholder = &script.effects[0];
        assert_eq!(placeholder.id, PLACEHOLDER_ID);
        assert!(!placeholder.enabled);
        assert_eq!(placeholder.params["exoScript"], Value::from("uf_easing"));
        assert_eq!(
            placeholder.params["exoScriptParameter"],
            Value::from("local P_VIEW=1;")
        );
        assert_eq!(
            placeholder.params["exoTracks"]["track0"],
            Value::from("0.00")
        );
        assert_eq!(placeholder.params["exoChecks"]["check0"], Value::from("1"));
        assert_eq!(outcome.report.placeholders, 1);
    }

    #[test]
    fn applies_the_requested_frame_and_layer_offset() {
        let (catalog, _) = EffectCatalog::load();
        let file = ExoFile::parse(&sample()).expect("sample object file");
        let placement = ImportPlacement {
            frame_offset: 25,
            layer_offset: 4,
            maximum_layers: 8,
        };
        let outcome = plan_import(&file, &scene(), &catalog, 1, placement);
        assert_eq!(outcome.clips[0].start, 35);
        assert_eq!(outcome.clips[0].layer, 6);
        assert_eq!(outcome.clips[1].layer, 4);
    }

    #[test]
    fn exported_objects_read_back_as_the_same_clips() {
        let (catalog, _) = EffectCatalog::load();
        let mut session = ProjectSession::blank_with(ProjectDefaults::default());
        let scene = session.document.scenes[0].clone();
        let file = ExoFile::parse(&sample()).expect("sample object file");
        let first = plan_import(&file, &scene, &catalog, 1, ImportPlacement::default());

        for clip in &first.clips {
            session.document.clips.push(clip.clone());
        }
        let (exported, report) = export_scene(&session.document, &scene, &catalog);
        assert_eq!(report.objects, 2, "both clips export");
        assert!(report.skipped.is_empty());

        let reparsed = ExoFile::parse(&exported.to_bytes()).expect("exported file parses");
        let second = plan_import(&reparsed, &scene, &catalog, 1, ImportPlacement::default());
        assert_eq!(second.clips.len(), first.clips.len());
        // The exporter writes layers from the bottom up, so compare by placement rather than
        // by the order the source file happened to use.
        let mut before = first.clips.clone();
        before.sort_by_key(|clip| (clip.layer, clip.start, clip.id));
        let mut after = second.clips.clone();
        after.sort_by_key(|clip| (clip.layer, clip.start, clip.id));
        for (before, after) in before.iter().zip(&after) {
            assert_eq!(before.clip_type, after.clip_type);
            assert_eq!(
                (before.start, before.duration, before.layer),
                (after.start, after.duration, after.layer)
            );
            assert_eq!(before.effects.len(), after.effects.len());
        }
        let text_before = before
            .iter()
            .find(|clip| clip.clip_type == "text")
            .expect("text clip");
        let text_after = after
            .iter()
            .find(|clip| clip.clip_type == "text")
            .expect("text clip");
        assert_eq!(
            text_before.effects[2].params["text"],
            text_after.effects[2].params["text"]
        );
        assert_eq!(
            text_before.effects[2].params["fontFamily"],
            text_after.effects[2].params["fontFamily"]
        );
        assert_eq!(
            text_before.effects[0].params["opacity"],
            text_after.effects[0].params["opacity"]
        );
        assert_eq!(
            text_before.effects[1].params["size"],
            text_after.effects[1].params["size"]
        );
        let script_before = before
            .iter()
            .find(|clip| clip.clip_type == "exo.自定义物件")
            .expect("script clip");
        let script_after = after
            .iter()
            .find(|clip| clip.clip_type == "exo.自定义物件")
            .expect("script clip");
        assert_eq!(
            script_before.effects[0].params["exoScript"],
            script_after.effects[0].params["exoScript"]
        );
        assert_eq!(
            script_before.effects[0].params["exoTracks"],
            script_after.effects[0].params["exoTracks"]
        );
    }
}
