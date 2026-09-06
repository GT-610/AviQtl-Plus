use crate::abi::{
    AviQtlAudioBakeInput, AviQtlAudioBakeOutput, AviQtlEffectParamEntry, AviQtlRenderBakeInput,
    AviQtlRenderBakeOutput, AviQtlSceneBakeCounts, STATUS_BUFFER_TOO_SMALL,
    STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK, STATUS_OVERLAPPING_BUFFERS,
    ranges_overlap, slice_is_valid,
};
use crate::keyframe_document::{evaluate_resolved_track, parse_hex_color, resolve_track};
use crate::policy::{PlaybackMode, playback_mode, resolve_video_time};
use crate::project::{
    AudioPluginDocument, ClipDocument, EffectDocument, ExtraFields, ProjectDocument,
};
use crate::timeline::{bake_audio, bake_render};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

const FRAME_BUCKET_SIZE: i32 = 120;
const DEFAULT_SPEED_PERCENT: f32 = 100.0;
const PARAM_TYPE_FLOAT: u8 = 0;
const PARAM_TYPE_COLOR: u8 = 4;

/// Error returned when a scene render plan cannot be built from a project snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneRenderPlanError {
    /// The requested scene does not exist in the project document.
    SceneNotFound(i32),
}

impl Display for SceneRenderPlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SceneNotFound(scene_id) => write!(formatter, "scene #{scene_id} does not exist"),
        }
    }
}

impl Error for SceneRenderPlanError {}

/// Rust-owned cached evaluator for one scene in a typed project document.
pub struct SceneRenderPlan {
    plan: BakePlan,
}

impl SceneRenderPlan {
    /// Builds the cached keyframe and timeline plan for one scene.
    pub fn from_document(
        document: &ProjectDocument,
        scene_id: i32,
    ) -> Result<Self, SceneRenderPlanError> {
        Ok(Self {
            plan: BakePlan::from_document(document, scene_id)?,
        })
    }

    /// Evaluates active render and audio layers at an absolute scene frame.
    pub fn evaluate(&mut self, current_frame: i32) -> SceneFramePlan {
        self.plan.compute_frame(EvaluationKey {
            current_frame,
            full_bake: false,
            prefetch_frames: 0,
        })
    }

    /// Returns the number of effects cached by this plan.
    pub fn effect_count(&self) -> usize {
        self.plan.effect_count
    }
}

/// Fully evaluated state needed to render and mix one scene frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneFramePlan {
    pub frame: i32,
    pub layers: Vec<RenderLayerPlan>,
    pub audio: Vec<AudioLayerPlan>,
    pub camera: Option<CameraRenderPlan>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraRenderPlan {
    pub position_x: f32,
    pub position_y: f32,
    pub position_z: f32,
    pub target_x: f32,
    pub target_y: f32,
    pub target_z: f32,
    pub roll_degrees: f32,
    pub field_of_view_degrees: f32,
}

/// Evaluated transform values for a render layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvaluatedTransform {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_x: f32,
    pub rotation_y: f32,
    pub rotation_z: f32,
    pub scale_percent: f32,
    pub opacity: f32,
}

impl Default for EvaluatedTransform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            scale_percent: 100.0,
            opacity: 1.0,
        }
    }
}

/// Parameters for one enabled effect after keyframe evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedEffect {
    pub effect_index: usize,
    pub id: String,
    pub params: BTreeMap<String, Value>,
}

/// Kind of media source attached to a render layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

/// Evaluated media playback state. Video time is resolved once source FPS is known.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaPlaybackPlan {
    pub kind: MediaKind,
    pub path: String,
    pub relative_frame: i32,
    pub direct_mode: bool,
    pub start_frame: f64,
    pub direct_frame: f64,
    pub speed_percent: f64,
    pub opacity: f32,
    fallback_fps: f64,
}

impl MediaPlaybackPlan {
    /// Resolves the requested source timestamp using decoder metadata when available.
    pub fn timestamp_seconds(&self, source_fps: f64) -> f64 {
        if self.kind == MediaKind::Image {
            return 0.0;
        }
        let source_fps = if source_fps.is_finite() && source_fps > 0.0 {
            source_fps
        } else {
            self.fallback_fps
        };
        resolve_video_time(
            self.relative_frame,
            source_fps,
            self.direct_mode,
            self.direct_frame,
            self.start_frame,
            self.speed_percent,
        )
    }
}

/// Evaluated visual state for one active timeline clip.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderLayerPlan {
    pub clip_id: i32,
    pub clip_type: String,
    pub timeline_layer: i32,
    pub start_frame: i32,
    pub duration_frames: i32,
    pub relative_frame: i32,
    pub clip_by_upper_object: bool,
    pub transform: EvaluatedTransform,
    pub effects: Vec<EvaluatedEffect>,
    pub media: Option<MediaPlaybackPlan>,
    pub shape: Option<ShapeRenderPlan>,
    pub text: Option<TextRenderPlan>,
    pub procedural: Option<ProceduralObjectRenderPlan>,
    pub nested_scene: Option<NestedSceneRenderPlan>,
}

/// Evaluated playback state for a scene object embedded in another scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NestedSceneRenderPlan {
    pub target_scene_id: i32,
    pub target_frame: i32,
    pub opacity: f32,
}

/// Procedural shape family used by AviQtl's built-in `rect` object.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ShapeKind {
    #[default]
    Polygon,
    Star,
    Pie,
    Arc,
    Donut,
}

/// Gradient mode used by a procedural shape fill.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ShapeGradientKind {
    #[default]
    Linear,
    Radial,
}

/// Straight-alpha color evaluated from an AviQtl color parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbaColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// Fully evaluated parameters for AviQtl's built-in procedural shape object.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRenderPlan {
    pub kind: ShapeKind,
    pub width: f32,
    pub height: f32,
    pub sides: u32,
    pub sweep_degrees: f32,
    pub corner_radius: f32,
    pub inner_radius_percent: f32,
    pub rotation_degrees: f32,
    pub normalize_vertices: bool,
    pub even_sides_half_step: bool,
    pub edge_padding: f32,
    pub fill_color: RgbaColor,
    pub use_gradient: bool,
    pub gradient_color: RgbaColor,
    pub gradient_kind: ShapeGradientKind,
    pub stroke_color: RgbaColor,
    pub stroke_width: f32,
    pub dash_length: f32,
    pub dash_space: f32,
    pub opacity: f32,
}

/// Horizontal alignment used by AviQtl's built-in text object.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    Right,
    #[default]
    Center,
    Justify,
}

/// Fully evaluated parameters for AviQtl's built-in text object.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRenderPlan {
    pub content: String,
    pub font_family: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub letter_spacing: f32,
    pub line_spacing: f32,
    pub alignment: TextAlignment,
    pub color: RgbaColor,
    pub outline_enabled: bool,
    pub outline_color: RgbaColor,
    pub outline_width: f32,
    pub shadow_enabled: bool,
    pub shadow_color: RgbaColor,
    pub shadow_offset_x: f32,
    pub shadow_offset_y: f32,
    pub background_enabled: bool,
    pub background_color: RgbaColor,
    pub background_radius: f32,
    pub background_padding_x: f32,
    pub background_padding_y: f32,
    pub opacity: f32,
}

/// CPU-rasterized custom object that mirrors one of the Qt Canvas implementations.
#[derive(Debug, Clone, PartialEq)]
pub enum ProceduralObjectRenderPlan {
    TrackLine(TrackLineRenderPlan),
    ParticleField(ParticleFieldRenderPlan),
    RadialLines(RadialLinesRenderPlan),
    LensFlare(LensFlareRenderPlan),
}

impl ProceduralObjectRenderPlan {
    pub fn opacity(&self) -> f32 {
        match self {
            Self::TrackLine(plan) => plan.opacity,
            Self::ParticleField(plan) => plan.opacity,
            Self::RadialLines(plan) => plan.opacity,
            Self::LensFlare(plan) => plan.opacity,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackLineRenderPlan {
    pub width: f32,
    pub height: f32,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub line_width: f32,
    pub dash_length: f32,
    pub dash_space: f32,
    pub arrow: bool,
    pub color: RgbaColor,
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParticleFieldRenderPlan {
    pub width: f32,
    pub height: f32,
    pub count: u32,
    pub speed: f32,
    pub particle_size: f32,
    pub spread: f32,
    pub seed: i32,
    pub color: RgbaColor,
    pub opacity: f32,
    pub relative_frame: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadialLinesRenderPlan {
    pub width: f32,
    pub height: f32,
    pub line_count: u32,
    pub min_length: f32,
    pub max_length: f32,
    pub thickness: f32,
    pub randomness: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub spin_speed: f32,
    pub seed: i32,
    pub color: RgbaColor,
    pub opacity: f32,
    pub relative_frame: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LensFlareRenderPlan {
    pub width: f32,
    pub height: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub radius: f32,
    pub strength: f32,
    pub ghosts: u32,
    pub color: RgbaColor,
    pub opacity: f32,
}

/// Evaluated audio state for one active audio or video clip.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioLayerPlan {
    pub clip_id: i32,
    pub source_path: Option<String>,
    pub start_frame: i32,
    pub duration_frames: i32,
    pub source_start_time: f32,
    pub playback_speed: f32,
    pub direct_time: f32,
    pub volume: f32,
    pub master_volume: f32,
    pub pan: f32,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub mute: bool,
    pub solo: bool,
    pub limiter: bool,
    pub direct_mode: bool,
    pub plugins: Vec<EvaluatedAudioPlugin>,
}

/// One enabled or bypassed audio-plugin instance with parameters evaluated for this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedAudioPlugin {
    pub id: String,
    pub enabled: bool,
    pub params: BTreeMap<String, Value>,
    pub extra: ExtraFields,
}

fn default_max_clip_id() -> i32 {
    4096
}

fn default_scene_height() -> i32 {
    1080
}

fn default_enabled() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SceneInput {
    fps: f64,
    #[serde(default = "default_scene_height")]
    height: i32,
    #[serde(default = "default_max_clip_id")]
    max_clip_id: i32,
    #[serde(default)]
    clips: Vec<ClipInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipInput {
    id: i32,
    #[serde(default)]
    layer: i32,
    #[serde(default)]
    start_frame: i32,
    #[serde(default)]
    duration_frames: i32,
    #[serde(default)]
    clip_by_upper_object: bool,
    #[serde(default, rename = "type")]
    clip_type: String,
    #[serde(default)]
    effects: Vec<EffectInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectInput {
    #[serde(default)]
    id: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    known: bool,
    #[serde(default)]
    params: BTreeMap<String, Value>,
    #[serde(default)]
    keyframes: BTreeMap<String, Vec<KeyframeInput>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyframeInput {
    #[serde(default)]
    frame: i32,
    #[serde(default)]
    value: Value,
    #[serde(default = "default_interpolation", rename = "interp")]
    interpolation: String,
    #[serde(default = "default_bzx1")]
    bzx1: f64,
    #[serde(default)]
    bzy1: f64,
    #[serde(default = "default_bzx2")]
    bzx2: f64,
    #[serde(default = "default_bzy2")]
    bzy2: f64,
}

fn default_interpolation() -> String {
    "linear".to_owned()
}

fn default_bzx1() -> f64 {
    0.33
}

fn default_bzx2() -> f64 {
    0.66
}

fn default_bzy2() -> f64 {
    1.0
}

fn payload(value: &Value) -> &Value {
    value
        .as_object()
        .filter(|object| object.contains_key("$aviqtlType"))
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
}

fn numeric_value(value: &Value) -> Option<f64> {
    let value = payload(value);
    value
        .as_f64()
        .or_else(|| value.as_bool().map(|value| if value { 1.0 } else { 0.0 }))
}

fn qt_double(value: Option<&Value>) -> f64 {
    let Some(value) = value.map(payload) else {
        return 0.0;
    };
    numeric_value(value)
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
        .unwrap_or(0.0)
}

fn qt_bool(value: Option<&Value>, fallback: bool) -> bool {
    let Some(value) = value.map(payload) else {
        return fallback;
    };
    if let Some(value) = value.as_bool() {
        return value;
    }
    if let Some(value) = value.as_f64() {
        return value != 0.0;
    }
    match value.as_str() {
        Some("true" | "1") => true,
        Some("false" | "0" | "") => false,
        Some(_) => true,
        None => false,
    }
}

fn point_value(point: &Value) -> Option<&Value> {
    point.as_object().and_then(|point| point.get("value"))
}

fn keyframe_point(keyframe: KeyframeInput) -> Value {
    let mut point = Map::new();
    point.insert("frame".to_owned(), Value::from(keyframe.frame));
    point.insert("value".to_owned(), keyframe.value);
    let custom = keyframe.interpolation == "custom";
    point.insert("interp".to_owned(), Value::String(keyframe.interpolation));
    if custom {
        point.insert(
            "points".to_owned(),
            Value::Array(
                [
                    keyframe.bzx1,
                    keyframe.bzy1,
                    keyframe.bzx2,
                    keyframe.bzy2,
                    1.0,
                    1.0,
                ]
                .into_iter()
                .map(Value::from)
                .collect(),
            ),
        );
    }
    Value::Object(point)
}

struct ResolvedTrack {
    fallback: Value,
    points: Vec<Value>,
    numeric: bool,
}

impl ResolvedTrack {
    fn new(keyframes: Vec<KeyframeInput>, fallback: Value, duration: i32) -> Self {
        let mut start = Map::new();
        start.insert("frame".to_owned(), Value::from(0));
        start.insert("value".to_owned(), fallback.clone());
        start.insert("interp".to_owned(), Value::String("none".to_owned()));
        let mut points = Vec::new();
        for keyframe in keyframes {
            let point = keyframe_point(keyframe);
            if point
                .as_object()
                .and_then(|point| point.get("frame"))
                .and_then(Value::as_i64)
                .is_some_and(|frame| frame <= 0)
            {
                start = point.as_object().cloned().unwrap_or_default();
            } else {
                points.push(point);
            }
        }
        let track = Value::Object(Map::from_iter([
            ("start".to_owned(), Value::Object(start)),
            ("points".to_owned(), Value::Array(points)),
        ]));
        Self::from_document_track(&track, fallback, duration)
    }

    fn from_document_track(track: &Value, fallback: Value, duration: i32) -> Self {
        let points = resolve_track(track, &fallback, duration);
        let numeric = points
            .iter()
            .all(|point| point_value(point).and_then(numeric_value).is_some());
        Self {
            fallback,
            points,
            numeric,
        }
    }

    fn evaluate(&self, frame: i32) -> Value {
        evaluate_resolved_track(&self.points, frame, &self.fallback)
    }
}

struct CachedEffect {
    id: String,
    enabled: bool,
    known: bool,
    params: BTreeMap<String, Value>,
    extra: ExtraFields,
    tracks: BTreeMap<String, ResolvedTrack>,
    all_keys: Vec<String>,
    numeric_track_count: usize,
    last_numeric_frame: Option<i32>,
    numeric_values: BTreeMap<String, f64>,
}

impl CachedEffect {
    fn new(input: EffectInput, duration: i32) -> Self {
        let mut all_keys: BTreeSet<String> = input.params.keys().cloned().collect();
        all_keys.insert("time".to_owned());
        let mut tracks = BTreeMap::new();
        for (name, keyframes) in input.keyframes {
            all_keys.insert(name.clone());
            let fallback = input.params.get(&name).cloned().unwrap_or(Value::Null);
            tracks.insert(name, ResolvedTrack::new(keyframes, fallback, duration));
        }
        let numeric_track_count = tracks.values().filter(|track| track.numeric).count();
        Self {
            id: input.id,
            enabled: input.enabled,
            known: input.known,
            params: input.params,
            extra: ExtraFields::new(),
            tracks,
            all_keys: all_keys.into_iter().collect(),
            numeric_track_count,
            last_numeric_frame: None,
            numeric_values: BTreeMap::new(),
        }
    }

    fn from_document(effect: &EffectDocument, duration: i32) -> Self {
        let params = effect
            .params
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut all_keys: BTreeSet<String> = params.keys().cloned().collect();
        all_keys.insert("time".to_owned());
        let mut tracks = BTreeMap::new();
        if let Some(keyframes) = &effect.keyframes {
            for (name, track) in keyframes {
                all_keys.insert(name.clone());
                let fallback = params.get(name).cloned().unwrap_or(Value::Null);
                tracks.insert(
                    name.clone(),
                    ResolvedTrack::from_document_track(track, fallback, duration),
                );
            }
        }
        let numeric_track_count = tracks.values().filter(|track| track.numeric).count();
        Self {
            id: effect.id.clone(),
            enabled: effect.enabled,
            known: true,
            params,
            extra: effect.extra.clone(),
            tracks,
            all_keys: all_keys.into_iter().collect(),
            numeric_track_count,
            last_numeric_frame: None,
            numeric_values: BTreeMap::new(),
        }
    }

    fn from_audio_plugin(plugin: &AudioPluginDocument, duration: i32) -> Self {
        let params = plugin
            .params
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut all_keys: BTreeSet<String> = params.keys().cloned().collect();
        let mut tracks = BTreeMap::new();
        if let Some(keyframes) = &plugin.keyframes {
            for (name, track) in keyframes {
                all_keys.insert(name.clone());
                let fallback = params.get(name).cloned().unwrap_or(Value::Null);
                tracks.insert(
                    name.clone(),
                    ResolvedTrack::from_document_track(track, fallback, duration),
                );
            }
        }
        let numeric_track_count = tracks.values().filter(|track| track.numeric).count();
        Self {
            id: plugin.id.clone(),
            enabled: plugin.enabled,
            known: true,
            params,
            extra: plugin.extra.clone(),
            tracks,
            all_keys: all_keys.into_iter().collect(),
            numeric_track_count,
            last_numeric_frame: None,
            numeric_values: BTreeMap::new(),
        }
    }

    fn prepare_numeric(&mut self, frame: i32, counts: &mut AviQtlSceneBakeCounts) {
        if self.numeric_track_count == 0 || self.last_numeric_frame == Some(frame) {
            return;
        }
        self.numeric_values.clear();
        for (name, track) in &self.tracks {
            if !track.numeric {
                continue;
            }
            let value = numeric_value(&track.evaluate(frame)).unwrap_or(0.0);
            self.numeric_values.insert(name.clone(), value);
        }
        self.last_numeric_frame = Some(frame);
        counts.numeric_batch_calls = counts.numeric_batch_calls.saturating_add(1);
        counts.numeric_track_count = counts
            .numeric_track_count
            .saturating_add(self.numeric_track_count as u64);
    }

    fn value(&self, name: &str, frame: i32) -> Value {
        if name == "time" {
            return Value::from(frame);
        }
        if let Some(value) = self.numeric_values.get(name) {
            return Value::from(*value);
        }
        if let Some(track) = self.tracks.get(name) {
            return track.evaluate(frame);
        }
        self.params.get(name).cloned().unwrap_or(Value::Null)
    }

    fn float(&self, name: &str, frame: i32) -> f32 {
        qt_double(Some(&self.value(name, frame))) as f32
    }

    fn float_or(&self, name: &str, fallback: f32, frame: i32) -> f32 {
        if !self.params.contains_key(name) && !self.tracks.contains_key(name) {
            return fallback;
        }
        self.float(name, frame)
    }
}

struct CachedClip {
    clip_id: i32,
    layer: i32,
    start_frame: i32,
    duration_frames: i32,
    end_frame: i32,
    clip_by_upper_object: bool,
    clip_type: String,
    audio_plugins: Vec<CachedEffect>,
    effects: Vec<CachedEffect>,
}

#[derive(Debug, Clone, Copy)]
struct EvaluatedGroupControl {
    layer: i32,
    layer_count: i32,
    x: f64,
    y: f64,
    z: f64,
    scale: f64,
    rotation_x: f64,
    rotation_y: f64,
    rotation_z: f64,
    opacity: f32,
}

impl CachedClip {
    fn new(input: ClipInput) -> Self {
        let effects = input
            .effects
            .into_iter()
            .map(|effect| CachedEffect::new(effect, input.duration_frames))
            .collect();
        Self {
            clip_id: input.id,
            layer: input.layer,
            start_frame: input.start_frame,
            duration_frames: input.duration_frames,
            end_frame: input
                .start_frame
                .saturating_add(input.duration_frames.max(0)),
            clip_by_upper_object: input.clip_by_upper_object,
            clip_type: input.clip_type,
            audio_plugins: Vec::new(),
            effects,
        }
    }

    fn from_document(clip: &ClipDocument) -> Self {
        let audio_plugins = clip
            .audio_plugins
            .iter()
            .map(|plugin| CachedEffect::from_audio_plugin(plugin, clip.duration))
            .collect();
        let effects = clip
            .effects
            .iter()
            .map(|effect| CachedEffect::from_document(effect, clip.duration))
            .collect();
        Self {
            clip_id: clip.id,
            layer: clip.layer,
            start_frame: clip.start,
            duration_frames: clip.duration,
            end_frame: clip.start.saturating_add(clip.duration.max(0)),
            clip_by_upper_object: clip.clip_by_upper_object,
            clip_type: clip.clip_type.clone(),
            audio_plugins,
            effects,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EvaluationKey {
    current_frame: i32,
    full_bake: bool,
    prefetch_frames: i32,
}

struct BakeEvaluation {
    key: EvaluationKey,
    renders: Vec<AviQtlRenderBakeOutput>,
    audio: Vec<AviQtlAudioBakeOutput>,
    params: Vec<AviQtlEffectParamEntry>,
    counts: AviQtlSceneBakeCounts,
    frame: Option<SceneFramePlan>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvaluationOutput {
    Abi,
    TypedFrame,
}

struct BakePlan {
    fps: f64,
    scene_height: i32,
    clips: Vec<CachedClip>,
    scene_durations: HashMap<i32, i32>,
    time_buckets: HashMap<i32, Vec<usize>>,
    effect_count: usize,
    pending: Option<BakeEvaluation>,
}

impl BakePlan {
    fn parse(input: &[u8]) -> Result<Self, ()> {
        let input: SceneInput = serde_json::from_slice(input).map_err(|_| ())?;
        if input.max_clip_id <= 0 {
            return Err(());
        }
        let clips = input
            .clips
            .into_iter()
            .filter(|clip| clip.id >= 0 && clip.id < input.max_clip_id)
            .map(CachedClip::new)
            .collect();
        Ok(Self::from_clips(
            input.fps,
            input.height,
            clips,
            HashMap::new(),
        ))
    }

    fn from_document(
        document: &ProjectDocument,
        scene_id: i32,
    ) -> Result<Self, SceneRenderPlanError> {
        let scene = document
            .scenes
            .iter()
            .find(|scene| scene.id == scene_id)
            .ok_or(SceneRenderPlanError::SceneNotFound(scene_id))?;
        let clips = document
            .clips
            .iter()
            .filter(|clip| clip.scene_id == scene_id)
            .map(CachedClip::from_document)
            .collect();
        let scene_durations = document
            .scenes
            .iter()
            .map(|scene| (scene.id, scene.duration))
            .collect();
        Ok(Self::from_clips(
            scene.fps,
            scene.height,
            clips,
            scene_durations,
        ))
    }

    fn from_clips(
        fps: f64,
        scene_height: i32,
        clips: Vec<CachedClip>,
        scene_durations: HashMap<i32, i32>,
    ) -> Self {
        let mut time_buckets: HashMap<i32, Vec<usize>> = HashMap::new();
        let effect_count = clips.iter().map(|clip| clip.effects.len()).sum();
        for (cached_index, cached) in clips.iter().enumerate() {
            let first_bucket = cached.start_frame / FRAME_BUCKET_SIZE;
            let last_bucket = cached.end_frame.max(cached.start_frame) / FRAME_BUCKET_SIZE;
            for bucket in first_bucket..=last_bucket {
                time_buckets.entry(bucket).or_default().push(cached_index);
            }
        }
        Self {
            fps,
            scene_height,
            clips,
            scene_durations,
            time_buckets,
            effect_count,
            pending: None,
        }
    }

    fn selected_clips(&self, key: EvaluationKey) -> (Vec<usize>, u64) {
        if key.full_bake {
            return ((0..self.clips.len()).collect(), self.clips.len() as u64);
        }
        let range_start = key.current_frame.saturating_sub(key.prefetch_frames);
        let range_end = key.current_frame.saturating_add(key.prefetch_frames);
        let first_bucket = range_start / FRAME_BUCKET_SIZE;
        let last_bucket = range_end / FRAME_BUCKET_SIZE;
        let mut seen = HashSet::new();
        let mut selected = Vec::new();
        let mut visited = 0_u64;
        for bucket in first_bucket..=last_bucket {
            let Some(indices) = self.time_buckets.get(&bucket) else {
                continue;
            };
            for &index in indices {
                let clip = &self.clips[index];
                if !seen.insert(clip.clip_id) {
                    continue;
                }
                visited = visited.saturating_add(1);
                if clip.start_frame <= range_end && clip.end_frame >= range_start {
                    selected.push(index);
                }
            }
        }
        (selected, visited)
    }

    fn compute(&mut self, key: EvaluationKey) -> BakeEvaluation {
        self.compute_output(key, EvaluationOutput::Abi)
    }

    fn compute_frame(&mut self, key: EvaluationKey) -> SceneFramePlan {
        self.compute_output(key, EvaluationOutput::TypedFrame)
            .frame
            .expect("typed frame evaluation always produces a frame plan")
    }

    fn compute_output(&mut self, key: EvaluationKey, output: EvaluationOutput) -> BakeEvaluation {
        let capture_abi = output == EvaluationOutput::Abi;
        let capture_frame = output == EvaluationOutput::TypedFrame;
        let (selected, clips_visited) = self.selected_clips(key);
        let mut counts = AviQtlSceneBakeCounts {
            clips_visited,
            ..AviQtlSceneBakeCounts::default()
        };
        let mut renders = Vec::with_capacity(selected.len());
        let mut audio = Vec::new();
        let mut params = Vec::new();
        let mut frame_layers = Vec::new();
        let mut frame_audio = Vec::new();

        for &index in &selected {
            let clip = &mut self.clips[index];
            let relative_frame = key.current_frame.saturating_sub(clip.start_frame).max(0);
            for effect in &mut clip.effects {
                effect.prepare_numeric(relative_frame, &mut counts);
            }
            for plugin in &mut clip.audio_plugins {
                plugin.prepare_numeric(relative_frame, &mut counts);
            }
        }
        let mut group_controls = selected
            .iter()
            .filter_map(|&index| {
                let clip = &self.clips[index];
                (clip.start_frame <= key.current_frame && key.current_frame < clip.end_frame)
                    .then(|| evaluated_group_control(clip, key.current_frame))
                    .flatten()
            })
            .collect::<Vec<_>>();
        group_controls.sort_by_key(|control| control.layer);
        let camera = selected
            .iter()
            .filter_map(|&index| {
                let clip = &self.clips[index];
                (clip.start_frame <= key.current_frame && key.current_frame < clip.end_frame)
                    .then(|| evaluated_camera_control(clip, key.current_frame, self.scene_height))
                    .flatten()
                    .map(|camera| (clip.layer, camera))
            })
            .min_by_key(|(layer, _)| *layer)
            .map(|(_, camera)| camera);

        for index in selected {
            let clip = &mut self.clips[index];
            counts.selected_effect_count = counts
                .selected_effect_count
                .saturating_add(clip.effects.len() as u64);
            let relative_frame = key.current_frame.saturating_sub(clip.start_frame).max(0);

            if clip.clip_type == "audio" || clip.clip_type == "video" {
                let mut input = AviQtlAudioBakeInput {
                    clip_id: clip.clip_id,
                    start_frame: clip.start_frame,
                    duration_frames: clip.duration_frames,
                    has_audio_effect: 0,
                    fps: self.fps,
                    source_start_time: 0.0,
                    speed_percent: DEFAULT_SPEED_PERCENT,
                    direct_time: 0.0,
                    volume: 1.0,
                    master_volume: 1.0,
                    pan: 0.0,
                    fade_in_seconds: 0.0,
                    fade_out_seconds: 0.0,
                    direct_mode: 0,
                    mute: 0,
                    solo: 0,
                    limiter: 0,
                };
                if matches!(self.fps.partial_cmp(&0.0), Some(Ordering::Greater) | None)
                    && let Some(effect) = clip
                        .effects
                        .iter()
                        .find(|effect| effect.enabled && effect.id == "audio")
                {
                    input.has_audio_effect = 1;
                    input.direct_mode = u32::from(matches!(
                        effect
                            .params
                            .get("playMode")
                            .map(payload)
                            .and_then(Value::as_str)
                            .and_then(playback_mode),
                        Some(PlaybackMode::Direct)
                    ));
                    input.source_start_time = effect.float_or("startTime", 0.0, relative_frame);
                    input.speed_percent =
                        effect.float_or("speed", DEFAULT_SPEED_PERCENT, relative_frame);
                    input.direct_time = effect.float_or("directTime", 0.0, relative_frame);
                    input.volume = effect.float_or("volume", 1.0, relative_frame);
                    input.master_volume = effect.float_or("masterVolume", 1.0, relative_frame);
                    input.pan = effect.float_or("pan", 0.0, relative_frame);
                    input.fade_in_seconds = effect.float_or("fadeIn", 0.0, relative_frame);
                    input.fade_out_seconds = effect.float_or("fadeOut", 0.0, relative_frame);
                    input.mute = u32::from(qt_bool(effect.params.get("mute"), false));
                    input.solo = u32::from(qt_bool(effect.params.get("solo"), false));
                    input.limiter = u32::from(qt_bool(effect.params.get("limiter"), true));
                }
                let mut output = bake_audio(input);
                // Scene projection needs a stable entity key even when the legacy single-clip
                // kernel returns its invalid-fps sentinel payload.
                output.clip_id = clip.clip_id;
                if capture_frame
                    && clip.start_frame <= key.current_frame
                    && key.current_frame < clip.end_frame
                {
                    frame_audio.push(audio_layer_plan(
                        output,
                        audio_source_path(&clip.clip_type, &clip.effects, relative_frame),
                        evaluated_audio_plugins(&clip.audio_plugins, relative_frame),
                    ));
                }
                if capture_abi {
                    audio.push(output);
                }
            }

            let effect_start_index = params.len() as u32;
            let mut render_input = AviQtlRenderBakeInput {
                clip_id: clip.clip_id,
                layer: clip.layer,
                current_frame: key.current_frame,
                start_frame: clip.start_frame,
                duration_frames: clip.duration_frames,
                clip_by_upper_object: u32::from(clip.clip_by_upper_object),
                effect_count: clip.effects.len() as u16,
                reserved: 0,
                effect_start_index,
                has_transform: 0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                rotation_x: 0.0,
                rotation_y: 0.0,
                rotation_z: 0.0,
                scale: 100.0,
                opacity: 1.0,
            };
            let mut evaluated_effects = capture_frame.then(Vec::new);

            for (effect_index, effect) in clip.effects.iter().enumerate() {
                if !effect.enabled || !effect.known {
                    continue;
                }
                if effect.id == "transform" {
                    render_input.has_transform = 1;
                    render_input.x = effect.float("x", relative_frame);
                    render_input.y = effect.float("y", relative_frame);
                    render_input.z = effect.float("z", relative_frame);
                    render_input.rotation_x = effect.float("rotationX", relative_frame);
                    render_input.rotation_y = effect.float("rotationY", relative_frame);
                    render_input.rotation_z = effect.float("rotationZ", relative_frame);
                    render_input.scale = effect.float("scale", relative_frame);
                    render_input.opacity = effect.float("opacity", relative_frame);
                }
                let mut evaluated_params = capture_frame.then(BTreeMap::new);
                for name in &effect.all_keys {
                    let evaluated = effect.value(name, relative_frame);
                    if capture_abi {
                        params.push(pack_parameter(
                            clip.clip_id,
                            effect_index as u8,
                            name,
                            &evaluated,
                        ));
                    }
                    if let Some(evaluated_params) = &mut evaluated_params {
                        evaluated_params.insert(name.clone(), evaluated);
                    }
                }
                if let (Some(evaluated_effects), Some(evaluated_params)) =
                    (&mut evaluated_effects, evaluated_params)
                {
                    evaluated_effects.push(EvaluatedEffect {
                        effect_index,
                        id: effect.id.clone(),
                        params: evaluated_params,
                    });
                }
            }
            let render = bake_render(render_input);
            if capture_frame
                && clip.start_frame <= key.current_frame
                && key.current_frame < clip.end_frame
            {
                let evaluated_effects =
                    evaluated_effects.expect("typed evaluation collects effect parameters");
                let mut transform = EvaluatedTransform {
                    x: render.x,
                    y: render.y,
                    z: render.z,
                    rotation_x: render.rotation_x,
                    rotation_y: render.rotation_y,
                    rotation_z: render.rotation_z,
                    scale_percent: render.scale_x * 100.0,
                    opacity: render.opacity,
                };
                apply_group_controls(&mut transform, clip.layer, &group_controls);
                let media = media_playback_plan(
                    &clip.clip_type,
                    relative_frame,
                    self.fps,
                    &evaluated_effects,
                );
                let shape = shape_render_plan(&clip.clip_type, &evaluated_effects);
                let text = text_render_plan(
                    &clip.clip_type,
                    relative_frame,
                    clip.duration_frames,
                    self.fps,
                    &evaluated_effects,
                );
                let procedural = procedural_object_render_plan(
                    &clip.clip_type,
                    relative_frame,
                    &evaluated_effects,
                );
                let nested_scene = nested_scene_render_plan(
                    &clip.clip_type,
                    relative_frame,
                    &evaluated_effects,
                    &self.scene_durations,
                );
                frame_layers.push(RenderLayerPlan {
                    clip_id: clip.clip_id,
                    clip_type: clip.clip_type.clone(),
                    timeline_layer: clip.layer,
                    start_frame: clip.start_frame,
                    duration_frames: clip.duration_frames,
                    relative_frame,
                    clip_by_upper_object: clip.clip_by_upper_object,
                    transform,
                    effects: evaluated_effects,
                    media,
                    shape,
                    text,
                    procedural,
                    nested_scene,
                });
            }
            if capture_abi {
                renders.push(render);
            }
        }

        counts.render_count = renders.len();
        counts.audio_count = audio.len();
        counts.param_count = params.len();
        BakeEvaluation {
            key,
            renders,
            audio,
            params,
            counts,
            frame: capture_frame.then_some(SceneFramePlan {
                frame: key.current_frame,
                layers: frame_layers,
                audio: frame_audio,
                camera,
            }),
        }
    }
}

fn value_as_f64(value: Option<&Value>, fallback: f64) -> f64 {
    value
        .map(payload)
        .and_then(|value| {
            numeric_value(value).or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}

fn value_as_bool(value: Option<&Value>, fallback: bool) -> bool {
    value
        .map(payload)
        .and_then(|value| {
            value.as_bool().or_else(|| {
                value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .map(|value| value != 0.0)
            })
        })
        .unwrap_or(fallback)
}

fn value_as_string(value: Option<&Value>, fallback: &str) -> String {
    value
        .map(payload)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

fn value_as_color(value: Option<&Value>, fallback: &str) -> RgbaColor {
    let [alpha, red, green, blue] = value
        .map(payload)
        .and_then(Value::as_str)
        .and_then(parse_hex_color)
        .or_else(|| parse_hex_color(fallback))
        .expect("built-in shape colors are valid");
    RgbaColor {
        red,
        green,
        blue,
        alpha,
    }
}

fn shape_render_plan(clip_type: &str, effects: &[EvaluatedEffect]) -> Option<ShapeRenderPlan> {
    let effect = effects.iter().find(|effect| effect.id == clip_type)?;
    let value = |name| effect.params.get(name);
    let (
        kind,
        width_default,
        height_default,
        sides,
        sweep_degrees,
        normalize_vertices,
        even_sides_half_step,
        edge_padding,
    ) = match clip_type {
        "rect" => {
            let kind = match value("shapeType")
                .map(payload)
                .and_then(Value::as_str)
                .unwrap_or("polygon")
            {
                "star" => ShapeKind::Star,
                "pie" => ShapeKind::Pie,
                "arc" => ShapeKind::Arc,
                "donut" => ShapeKind::Donut,
                _ => ShapeKind::Polygon,
            };
            let sides = value_as_f64(value("sides"), 4.0).round().clamp(3.0, 360.0) as u32;
            (kind, 200.0, 200.0, sides, sides as f32, true, true, 2.0)
        }
        "polygon_shape" => (
            ShapeKind::Polygon,
            1_920.0,
            1_080.0,
            value_as_f64(value("sides"), 6.0).round().clamp(3.0, 64.0) as u32,
            360.0,
            false,
            false,
            4.0,
        ),
        "pie_shape" => (
            ShapeKind::Pie,
            1_920.0,
            1_080.0,
            3,
            value_as_f64(value("angle"), 180.0).clamp(1.0, 360.0) as f32,
            false,
            false,
            4.0,
        ),
        _ => return None,
    };
    Some(ShapeRenderPlan {
        kind,
        width: value_as_f64(value("sizeW"), width_default).max(0.0) as f32,
        height: value_as_f64(value("sizeH"), height_default).max(0.0) as f32,
        sides,
        sweep_degrees,
        corner_radius: value_as_f64(value("cornerRadius"), 0.0).max(0.0) as f32,
        inner_radius_percent: value_as_f64(value("innerRadius"), 50.0).clamp(0.0, 100.0) as f32,
        rotation_degrees: value_as_f64(value("rotation"), 0.0) as f32,
        normalize_vertices,
        even_sides_half_step,
        edge_padding,
        fill_color: value_as_color(value("color"), "#66aa99"),
        use_gradient: value_as_bool(value("useGradient"), false),
        gradient_color: value_as_color(value("gradientColor2"), "#ffffff"),
        gradient_kind: if value_as_f64(value("gradientType"), 0.0).round() == 1.0 {
            ShapeGradientKind::Radial
        } else {
            ShapeGradientKind::Linear
        },
        stroke_color: value_as_color(value("strokeColor"), "#ffffff"),
        stroke_width: value_as_f64(value("strokeWidth"), 0.0).max(0.0) as f32,
        dash_length: value_as_f64(value("dashLength"), 0.0).max(0.0) as f32,
        dash_space: value_as_f64(value("dashSpace"), 0.0).max(0.0) as f32,
        opacity: value_as_f64(value("opacity"), 1.0).clamp(0.0, 1.0) as f32,
    })
}

fn text_render_plan(
    clip_type: &str,
    relative_frame: i32,
    duration_frames: i32,
    fps: f64,
    effects: &[EvaluatedEffect],
) -> Option<TextRenderPlan> {
    let effect = effects.iter().find(|effect| effect.id == clip_type)?;
    let value = |name| effect.params.get(name);
    if clip_type == "counter" {
        let outline_width = value_as_f64(value("outlineWidth"), 2.0).clamp(0.0, 512.0) as f32;
        return Some(TextRenderPlan {
            content: counter_text(&effect.params, relative_frame, duration_frames, fps),
            font_family: value_as_string(value("fontFamily"), "sans-serif"),
            font_size: value_as_f64(value("fontSize"), 64.0).clamp(1.0, 4_096.0) as f32,
            bold: false,
            italic: false,
            letter_spacing: 0.0,
            line_spacing: 0.0,
            alignment: TextAlignment::Center,
            color: value_as_color(value("color"), "#ffffff"),
            outline_enabled: outline_width > 0.0,
            outline_color: value_as_color(value("outlineColor"), "#000000"),
            outline_width,
            shadow_enabled: false,
            shadow_color: value_as_color(None, "#00000000"),
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            background_enabled: false,
            background_color: value_as_color(None, "#00000000"),
            background_radius: 0.0,
            background_padding_x: 0.0,
            background_padding_y: 0.0,
            opacity: value_as_f64(value("opacity"), 1.0).clamp(0.0, 1.0) as f32,
        });
    }
    if clip_type != "text" {
        return None;
    }
    let alignment = match value_as_f64(value("alignment"), 4.0).round() as i32 {
        1 => TextAlignment::Left,
        2 => TextAlignment::Right,
        8 => TextAlignment::Justify,
        _ => TextAlignment::Center,
    };
    Some(TextRenderPlan {
        content: value_as_string(value("text"), "テキスト"),
        font_family: value_as_string(value("fontFamily"), "sans-serif"),
        font_size: value_as_f64(value("fontSize"), 48.0).clamp(1.0, 4_096.0) as f32,
        bold: value_as_bool(value("fontBold"), false),
        italic: value_as_bool(value("fontItalic"), false),
        letter_spacing: value_as_f64(value("letterSpacing"), 0.0) as f32,
        line_spacing: value_as_f64(value("lineSpacing"), 0.0).max(0.0) as f32,
        alignment,
        color: value_as_color(value("color"), "#ffffff"),
        outline_enabled: value_as_bool(value("outlineEnabled"), false),
        outline_color: value_as_color(value("outlineColor"), "#000000"),
        outline_width: value_as_f64(value("outlineWidth"), 2.0).clamp(0.0, 512.0) as f32,
        shadow_enabled: value_as_bool(value("shadowEnabled"), false),
        shadow_color: value_as_color(value("shadowColor"), "#80000000"),
        shadow_offset_x: value_as_f64(value("shadowOffsetX"), 5.0) as f32,
        shadow_offset_y: value_as_f64(value("shadowOffsetY"), 5.0) as f32,
        background_enabled: value_as_bool(value("bgEnabled"), false),
        background_color: value_as_color(value("backgroundColor"), "#80000000"),
        background_radius: value_as_f64(value("backgroundRadius"), 10.0).max(0.0) as f32,
        background_padding_x: value_as_f64(value("backgroundPaddingX"), 20.0).max(0.0) as f32,
        background_padding_y: value_as_f64(value("backgroundPaddingY"), 10.0).max(0.0) as f32,
        opacity: 1.0,
    })
}

fn counter_text(
    params: &BTreeMap<String, Value>,
    relative_frame: i32,
    duration_frames: i32,
    fps: f64,
) -> String {
    let value = |name| params.get(name);
    let mode = value_as_string(value("mode"), "frame");
    let counter_value = match mode.as_str() {
        "time" => f64::from(relative_frame) / fps.max(0.001),
        "value" => {
            let progress = if duration_frames > 0 {
                (f64::from(relative_frame) / f64::from(duration_frames)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let start = value_as_f64(value("startValue"), 0.0);
            let end = value_as_f64(value("endValue"), 100.0);
            start + (end - start) * progress
        }
        _ => f64::from(relative_frame),
    };
    let decimals = value_as_f64(value("decimals"), 0.0)
        .round()
        .clamp(0.0, 100.0) as usize;
    let digits = value_as_f64(value("digits"), 0.0)
        .round()
        .clamp(0.0, 4_096.0) as usize;
    let mut number = format!("{counter_value:.decimals$}");
    let negative = number.starts_with('-');
    let body = number.strip_prefix('-').unwrap_or(&number);
    let (integer, fraction) = body.split_once('.').unwrap_or((body, ""));
    if digits > integer.len() {
        let mut padded = String::with_capacity(number.len() + digits - integer.len());
        if negative {
            padded.push('-');
        }
        padded.extend(std::iter::repeat_n('0', digits - integer.len()));
        padded.push_str(integer);
        if decimals > 0 {
            padded.push('.');
            padded.push_str(fraction);
        }
        number = padded;
    }
    format!(
        "{}{}{}",
        value_as_string(value("prefix"), ""),
        number,
        value_as_string(value("suffix"), "")
    )
}

fn procedural_object_render_plan(
    clip_type: &str,
    relative_frame: i32,
    effects: &[EvaluatedEffect],
) -> Option<ProceduralObjectRenderPlan> {
    let effect = effects.iter().find(|effect| effect.id == clip_type)?;
    let value = |name| effect.params.get(name);
    let width = value_as_f64(value("sizeW"), 1_920.0).max(1.0) as f32;
    let height = value_as_f64(value("sizeH"), 1_080.0).max(1.0) as f32;
    let opacity = value_as_f64(value("opacity"), 1.0).clamp(0.0, 1.0) as f32;
    match clip_type {
        "track_line" => Some(ProceduralObjectRenderPlan::TrackLine(TrackLineRenderPlan {
            width,
            height,
            start_x: value_as_f64(value("startX"), -240.0) as f32,
            start_y: value_as_f64(value("startY"), 120.0) as f32,
            end_x: value_as_f64(value("endX"), 240.0) as f32,
            end_y: value_as_f64(value("endY"), -120.0) as f32,
            line_width: value_as_f64(value("lineWidth"), 8.0) as f32,
            dash_length: value_as_f64(value("dashLength"), 0.0) as f32,
            dash_space: value_as_f64(value("dashSpace"), 12.0) as f32,
            arrow: value_as_bool(value("arrow"), true),
            color: value_as_color(value("color"), "#ffffff"),
            opacity,
        })),
        "star" => Some(ProceduralObjectRenderPlan::ParticleField(
            ParticleFieldRenderPlan {
                width,
                height,
                count: value_as_f64(value("count"), 120.0)
                    .round()
                    .clamp(1.0, 2_000.0) as u32,
                speed: value_as_f64(value("speed"), 1.0) as f32,
                particle_size: value_as_f64(value("particleSize"), 4.0) as f32,
                spread: value_as_f64(value("spread"), 1.0) as f32,
                seed: value_as_f64(value("seed"), 1.0).round() as i32,
                color: value_as_color(value("color"), "#ffffff"),
                opacity,
                relative_frame,
            },
        )),
        "radial_lines" => Some(ProceduralObjectRenderPlan::RadialLines(
            RadialLinesRenderPlan {
                width,
                height,
                line_count: value_as_f64(value("lineCount"), 128.0)
                    .round()
                    .clamp(1.0, 512.0) as u32,
                min_length: value_as_f64(value("minLength"), 760.0) as f32,
                max_length: value_as_f64(value("maxLength"), 1_500.0) as f32,
                thickness: value_as_f64(value("thickness"), 5.0) as f32,
                randomness: value_as_f64(value("randomness"), 0.75) as f32,
                center_x: value_as_f64(value("centerX"), 0.0) as f32,
                center_y: value_as_f64(value("centerY"), 0.0) as f32,
                spin_speed: value_as_f64(value("spinSpeed"), 0.0) as f32,
                seed: value_as_f64(value("seed"), 1.0).round() as i32,
                color: value_as_color(value("color"), "#ffffff"),
                opacity,
                relative_frame,
            },
        )),
        "flare" | "lens_flare_object" => {
            let (default_radius, default_strength, default_ghosts, default_color) =
                if clip_type == "flare" {
                    (90.0, 1.3, 0.0, "#ffd1a3")
                } else {
                    (180.0, 1.0, 4.0, "#fff2aa")
                };
            Some(ProceduralObjectRenderPlan::LensFlare(LensFlareRenderPlan {
                width,
                height,
                center_x: value_as_f64(value("centerX"), 0.0) as f32,
                center_y: value_as_f64(value("centerY"), 0.0) as f32,
                radius: value_as_f64(value("radius"), default_radius).max(0.0) as f32,
                strength: value_as_f64(value("strength"), default_strength).max(0.0) as f32,
                ghosts: value_as_f64(value("ghosts"), default_ghosts)
                    .round()
                    .clamp(0.0, 16.0) as u32,
                color: value_as_color(value("color"), default_color),
                opacity,
            }))
        }
        _ => None,
    }
}

fn evaluated_group_control(clip: &CachedClip, current_frame: i32) -> Option<EvaluatedGroupControl> {
    if clip.clip_type != "GroupControl" {
        return None;
    }
    let relative_frame = current_frame.saturating_sub(clip.start_frame).max(0);
    let effect = clip
        .effects
        .iter()
        .find(|effect| effect.enabled && effect.id == "GroupControl")?;
    Some(EvaluatedGroupControl {
        layer: clip.layer,
        layer_count: effect
            .float_or("layerCount", 1.0, relative_frame)
            .round()
            .max(1.0) as i32,
        x: f64::from(effect.float_or("x", 0.0, relative_frame)),
        y: f64::from(effect.float_or("y", 0.0, relative_frame)),
        z: f64::from(effect.float_or("z", 0.0, relative_frame)),
        scale: f64::from(effect.float_or("scale", 100.0, relative_frame)) / 100.0,
        rotation_x: f64::from(effect.float_or("rotationX", 0.0, relative_frame)),
        rotation_y: f64::from(effect.float_or("rotationY", 0.0, relative_frame)),
        rotation_z: f64::from(effect.float_or("rotationZ", 0.0, relative_frame)),
        opacity: effect.float_or("opacity", 1.0, relative_frame),
    })
}

fn evaluated_camera_control(
    clip: &CachedClip,
    current_frame: i32,
    scene_height: i32,
) -> Option<CameraRenderPlan> {
    if !matches!(clip.clip_type.as_str(), "camera_control" | "camera") {
        return None;
    }
    let relative_frame = current_frame.saturating_sub(clip.start_frame).max(0);
    let current = clip
        .effects
        .iter()
        .find(|effect| effect.enabled && effect.id == "camera_control");
    let legacy = clip
        .effects
        .iter()
        .find(|effect| effect.enabled && effect.id == "camera");
    current.or(legacy)?;
    let parameter = |name: &str, fallback: f32| {
        current
            .filter(|effect| effect.params.contains_key(name) || effect.tracks.contains_key(name))
            .or_else(|| {
                legacy.filter(|effect| {
                    effect.params.contains_key(name) || effect.tracks.contains_key(name)
                })
            })
            .map_or(fallback, |effect| effect.float(name, relative_frame))
    };
    let field_of_view_degrees = parameter("fov", 30.0).clamp(1.0, 170.0);
    let default_distance =
        scene_height.max(1) as f32 / (2.0 * (field_of_view_degrees.to_radians() / 2.0).tan());
    Some(CameraRenderPlan {
        position_x: parameter("x", 0.0),
        position_y: -parameter("y", 0.0),
        position_z: default_distance + parameter("z", 0.0),
        target_x: parameter("tx", 0.0),
        target_y: -parameter("ty", 0.0),
        target_z: parameter("tz", 0.0),
        roll_degrees: parameter("roll", 0.0),
        field_of_view_degrees,
    })
}

fn apply_group_controls(
    transform: &mut EvaluatedTransform,
    layer: i32,
    controls: &[EvaluatedGroupControl],
) {
    let active = controls
        .iter()
        .filter(|control| {
            control.layer < layer && layer <= control.layer.saturating_add(control.layer_count)
        })
        .collect::<Vec<_>>();
    if active.is_empty() {
        return;
    }
    let mut matrix = identity_matrix();
    for control in active {
        matrix = multiply_matrix(matrix, translation_matrix(control.x, control.y, control.z));
        matrix = multiply_matrix(matrix, rotation_x_matrix(control.rotation_x.to_radians()));
        matrix = multiply_matrix(matrix, rotation_y_matrix(control.rotation_y.to_radians()));
        matrix = multiply_matrix(matrix, rotation_z_matrix(control.rotation_z.to_radians()));
        matrix = multiply_matrix(
            matrix,
            scale_matrix(control.scale, control.scale, control.scale),
        );
        transform.rotation_x += control.rotation_x as f32;
        transform.rotation_y += control.rotation_y as f32;
        transform.rotation_z += control.rotation_z as f32;
        transform.opacity *= control.opacity;
    }
    matrix = multiply_matrix(
        matrix,
        translation_matrix(
            f64::from(transform.x),
            f64::from(transform.y),
            f64::from(transform.z),
        ),
    );
    transform.x = matrix[12] as f32;
    transform.y = matrix[13] as f32;
    transform.z = matrix[14] as f32;
}

fn identity_matrix() -> [f64; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn multiply_matrix(left: [f64; 16], right: [f64; 16]) -> [f64; 16] {
    let mut result = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            result[column * 4 + row] = (0..4)
                .map(|index| left[index * 4 + row] * right[column * 4 + index])
                .sum();
        }
    }
    result
}

fn translation_matrix(x: f64, y: f64, z: f64) -> [f64; 16] {
    let mut matrix = identity_matrix();
    matrix[12] = x;
    matrix[13] = y;
    matrix[14] = z;
    matrix
}

fn scale_matrix(x: f64, y: f64, z: f64) -> [f64; 16] {
    [
        x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn rotation_x_matrix(radians: f64) -> [f64; 16] {
    let (sine, cosine) = radians.sin_cos();
    [
        1.0, 0.0, 0.0, 0.0, 0.0, cosine, sine, 0.0, 0.0, -sine, cosine, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn rotation_y_matrix(radians: f64) -> [f64; 16] {
    let (sine, cosine) = radians.sin_cos();
    [
        cosine, 0.0, -sine, 0.0, 0.0, 1.0, 0.0, 0.0, sine, 0.0, cosine, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn rotation_z_matrix(radians: f64) -> [f64; 16] {
    let (sine, cosine) = radians.sin_cos();
    [
        cosine, sine, 0.0, 0.0, -sine, cosine, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn nested_scene_render_plan(
    clip_type: &str,
    relative_frame: i32,
    effects: &[EvaluatedEffect],
    scene_durations: &HashMap<i32, i32>,
) -> Option<NestedSceneRenderPlan> {
    if clip_type != "scene" {
        return None;
    }
    let effect = effects.iter().find(|effect| effect.id == "scene")?;
    let value = |name| effect.params.get(name);
    let target_scene_id = value_as_f64(value("targetSceneId"), -1.0).round() as i32;
    let duration = *scene_durations.get(&target_scene_id)?;
    let speed = value_as_f64(value("speed"), 1.0);
    let offset = value_as_f64(value("offset"), 0.0).floor();
    let target_frame = (f64::from(relative_frame) * speed).floor() + offset;
    Some(NestedSceneRenderPlan {
        target_scene_id,
        target_frame: target_frame.clamp(0.0, f64::from(duration.saturating_sub(1).max(0))) as i32,
        opacity: value_as_f64(value("opacity"), 1.0).clamp(0.0, 1.0) as f32,
    })
}

fn media_playback_plan(
    clip_type: &str,
    relative_frame: i32,
    fallback_fps: f64,
    effects: &[EvaluatedEffect],
) -> Option<MediaPlaybackPlan> {
    let kind = match clip_type {
        "image" => MediaKind::Image,
        "video" => MediaKind::Video,
        _ => return None,
    };
    let effect = effects.iter().find(|effect| effect.id == clip_type)?;
    let path = effect
        .params
        .get("path")
        .map(payload)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())?
        .to_owned();
    let direct_mode = effect
        .params
        .get("playMode")
        .map(payload)
        .and_then(Value::as_str)
        .and_then(playback_mode)
        == Some(PlaybackMode::Direct);
    Some(MediaPlaybackPlan {
        kind,
        path,
        relative_frame,
        direct_mode,
        start_frame: value_as_f64(effect.params.get("startFrame"), 0.0),
        direct_frame: value_as_f64(effect.params.get("directFrame"), 0.0),
        speed_percent: value_as_f64(effect.params.get("speed"), 100.0),
        opacity: value_as_f64(effect.params.get("opacity"), 1.0) as f32,
        fallback_fps,
    })
}

fn audio_source_path(
    clip_type: &str,
    effects: &[CachedEffect],
    relative_frame: i32,
) -> Option<String> {
    let parameter = match clip_type {
        "audio" => "source",
        "video" => "path",
        _ => return None,
    };
    effects
        .iter()
        .find(|effect| effect.enabled && effect.id == clip_type)
        .map(|effect| effect.value(parameter, relative_frame))
        .as_ref()
        .map(payload)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

fn evaluated_audio_plugins(
    plugins: &[CachedEffect],
    relative_frame: i32,
) -> Vec<EvaluatedAudioPlugin> {
    plugins
        .iter()
        .map(|plugin| EvaluatedAudioPlugin {
            id: plugin.id.clone(),
            enabled: plugin.enabled,
            params: plugin
                .all_keys
                .iter()
                .map(|name| (name.clone(), plugin.value(name, relative_frame)))
                .collect(),
            extra: plugin.extra.clone(),
        })
        .collect()
}

fn audio_layer_plan(
    output: AviQtlAudioBakeOutput,
    source_path: Option<String>,
    plugins: Vec<EvaluatedAudioPlugin>,
) -> AudioLayerPlan {
    AudioLayerPlan {
        clip_id: output.clip_id,
        source_path,
        start_frame: output.start_frame,
        duration_frames: output.duration_frames,
        source_start_time: output.source_start_time,
        playback_speed: output.playback_speed,
        direct_time: output.direct_time,
        volume: output.volume,
        master_volume: output.master_volume,
        pan: output.pan,
        fade_in_seconds: output.fade_in_seconds,
        fade_out_seconds: output.fade_out_seconds,
        mute: output.mute != 0,
        solo: output.solo != 0,
        limiter: output.limiter != 0,
        direct_mode: output.direct_mode != 0,
        plugins,
    }
}

fn pack_parameter(
    clip_id: i32,
    effect_index: u8,
    name: &str,
    evaluated: &Value,
) -> AviQtlEffectParamEntry {
    let mut entry = AviQtlEffectParamEntry {
        clip_id: clip_id as u32,
        effect_index,
        ..AviQtlEffectParamEntry::default()
    };
    let bytes = name.as_bytes();
    let mut copy_length = bytes.len().min(entry.param_name.len() - 1);
    while copy_length > 0 && !name.is_char_boundary(copy_length) {
        copy_length -= 1;
    }
    entry.param_name[..copy_length].copy_from_slice(&bytes[..copy_length]);

    if let Some(color) = payload(evaluated).as_str().and_then(parse_hex_color) {
        entry.param_type = PARAM_TYPE_COLOR;
        entry.value = [
            f32::from(color[1]) / 255.0,
            f32::from(color[2]) / 255.0,
            f32::from(color[3]) / 255.0,
            f32::from(color[0]) / 255.0,
        ];
    } else {
        entry.param_type = PARAM_TYPE_FLOAT;
        entry.value[0] = qt_double(Some(evaluated)) as f32;
    }
    entry
}

pub struct AviQtlTimelineBakePlan {
    plan: BakePlan,
}

fn parse_plan(input: *const u8, input_length: usize) -> Result<BakePlan, u32> {
    if !slice_is_valid(input, input_length) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    let input = if input_length == 0 {
        &[]
    } else {
        // SAFETY: The readable byte range was validated by the caller-facing function.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    BakePlan::parse(input).map_err(|()| STATUS_INVALID_JSON)
}

/// Creates an opaque scene bake plan from a JSON snapshot.
///
/// # Safety
///
/// `input` must be readable for `input_length` bytes and `output_handle` writable for one pointer.
/// Their ranges must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_bake_plan_create(
    input: *const u8,
    input_length: usize,
    output_handle: *mut *mut AviQtlTimelineBakePlan,
) -> u32 {
    if !slice_is_valid(input, input_length) || !slice_is_valid(output_handle, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(input, input_length, output_handle, 1) {
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        Some(false) => {}
        None => return STATUS_INVALID_ARGUMENT,
    }
    let plan = match parse_plan(input, input_length) {
        Ok(plan) => plan,
        Err(status) => return status,
    };
    let handle = Box::into_raw(Box::new(AviQtlTimelineBakePlan { plan }));
    // SAFETY: The output pointer was validated and is disjoint from the input range.
    unsafe { output_handle.write(handle) };
    STATUS_OK
}

/// Destroys an opaque scene bake plan. A null handle is accepted.
///
/// # Safety
///
/// A non-null handle must have been returned by `aviqtl_timeline_bake_plan_create`, must not have
/// been destroyed already, and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_bake_plan_destroy(handle: *mut AviQtlTimelineBakePlan) {
    if !handle.is_null() {
        // SAFETY: The caller owns this unique allocation under the contract above.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Replaces the contents of an existing scene bake plan from a JSON snapshot.
///
/// # Safety
///
/// `handle` must be a live unique plan and `input` readable for `input_length` bytes. The input
/// range must not overlap the plan allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_bake_plan_reset(
    handle: *mut AviQtlTimelineBakePlan,
    input: *const u8,
    input_length: usize,
) -> u32 {
    if !slice_is_valid(handle, 1) || !slice_is_valid(input, input_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(handle, 1, input, input_length) {
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        Some(false) => {}
        None => return STATUS_INVALID_ARGUMENT,
    }
    let plan = match parse_plan(input, input_length) {
        Ok(plan) => plan,
        Err(status) => return status,
    };
    // SAFETY: The handle was validated, is uniquely borrowed by the caller, and input parsing is
    // complete before mutation.
    unsafe { (*handle).plan = plan };
    STATUS_OK
}

/// Returns the number of cached effects in a scene plan, or zero for a null/misaligned handle.
///
/// # Safety
///
/// A non-null handle must reference a live plan for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_timeline_bake_plan_effect_count(
    handle: *mut AviQtlTimelineBakePlan,
) -> usize {
    if !slice_is_valid(handle, 1) {
        return 0;
    }
    // SAFETY: The handle was validated and the caller keeps it alive.
    unsafe { (*handle).plan.effect_count }
}

struct OutputRanges {
    handle: *mut AviQtlTimelineBakePlan,
    render_output: *mut AviQtlRenderBakeOutput,
    render_capacity: usize,
    audio_output: *mut AviQtlAudioBakeOutput,
    audio_capacity: usize,
    param_output: *mut AviQtlEffectParamEntry,
    param_capacity: usize,
    counts: *mut AviQtlSceneBakeCounts,
}

fn output_ranges_are_valid(ranges: OutputRanges) -> Result<(), u32> {
    if !slice_is_valid(ranges.handle, 1)
        || !slice_is_valid(ranges.render_output, ranges.render_capacity)
        || !slice_is_valid(ranges.audio_output, ranges.audio_capacity)
        || !slice_is_valid(ranges.param_output, ranges.param_capacity)
        || !slice_is_valid(ranges.counts, 1)
    {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    let overlaps = [
        ranges_overlap(
            ranges.handle,
            1,
            ranges.render_output,
            ranges.render_capacity,
        ),
        ranges_overlap(ranges.handle, 1, ranges.audio_output, ranges.audio_capacity),
        ranges_overlap(ranges.handle, 1, ranges.param_output, ranges.param_capacity),
        ranges_overlap(ranges.handle, 1, ranges.counts, 1),
        ranges_overlap(
            ranges.render_output,
            ranges.render_capacity,
            ranges.audio_output,
            ranges.audio_capacity,
        ),
        ranges_overlap(
            ranges.render_output,
            ranges.render_capacity,
            ranges.param_output,
            ranges.param_capacity,
        ),
        ranges_overlap(
            ranges.render_output,
            ranges.render_capacity,
            ranges.counts,
            1,
        ),
        ranges_overlap(
            ranges.audio_output,
            ranges.audio_capacity,
            ranges.param_output,
            ranges.param_capacity,
        ),
        ranges_overlap(ranges.audio_output, ranges.audio_capacity, ranges.counts, 1),
        ranges_overlap(ranges.param_output, ranges.param_capacity, ranges.counts, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return Err(STATUS_OVERLAPPING_BUFFERS);
    }
    Ok(())
}

/// Evaluates a scene plan into caller-owned POD buffers.
///
/// # Safety
///
/// `handle` must be a live unique plan. Every non-empty output range must be writable and all
/// output ranges, the counts value, and the plan allocation must be mutually disjoint. Null array
/// pointers are accepted only with zero capacity. Required counts are always reported after valid
/// arguments; insufficient capacity never writes a partial array.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn aviqtl_timeline_bake_plan_evaluate(
    handle: *mut AviQtlTimelineBakePlan,
    current_frame: i32,
    full_bake: u32,
    prefetch_frames: i32,
    render_output: *mut AviQtlRenderBakeOutput,
    render_capacity: usize,
    audio_output: *mut AviQtlAudioBakeOutput,
    audio_capacity: usize,
    param_output: *mut AviQtlEffectParamEntry,
    param_capacity: usize,
    counts: *mut AviQtlSceneBakeCounts,
) -> u32 {
    if let Err(status) = output_ranges_are_valid(OutputRanges {
        handle,
        render_output,
        render_capacity,
        audio_output,
        audio_capacity,
        param_output,
        param_capacity,
        counts,
    }) {
        return status;
    }
    let key = EvaluationKey {
        current_frame,
        full_bake: full_bake != 0,
        prefetch_frames: prefetch_frames.max(0),
    };
    // SAFETY: The handle was validated and is uniquely owned for this call.
    let plan = unsafe { &mut (*handle).plan };
    if plan
        .pending
        .as_ref()
        .is_none_or(|evaluation| evaluation.key != key)
    {
        plan.pending = Some(plan.compute(key));
    }
    let evaluation = plan
        .pending
        .as_ref()
        .expect("pending evaluation was populated");
    // SAFETY: The counts output was validated and is disjoint from every array.
    unsafe { counts.write(evaluation.counts) };
    if render_capacity < evaluation.renders.len()
        || audio_capacity < evaluation.audio.len()
        || param_capacity < evaluation.params.len()
    {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !evaluation.renders.is_empty() {
        // SAFETY: Capacity, alignment, validity, and disjointness were checked above.
        unsafe {
            std::slice::from_raw_parts_mut(render_output, render_capacity)
                [..evaluation.renders.len()]
                .copy_from_slice(&evaluation.renders)
        };
    }
    if !evaluation.audio.is_empty() {
        // SAFETY: Capacity, alignment, validity, and disjointness were checked above.
        unsafe {
            std::slice::from_raw_parts_mut(audio_output, audio_capacity)[..evaluation.audio.len()]
                .copy_from_slice(&evaluation.audio)
        };
    }
    if !evaluation.params.is_empty() {
        // SAFETY: Capacity, alignment, validity, and disjointness were checked above.
        unsafe {
            std::slice::from_raw_parts_mut(param_output, param_capacity)[..evaluation.params.len()]
                .copy_from_slice(&evaluation.params)
        };
    }
    plan.pending = None;
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scene_json() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "fps": 60.0,
            "maxClipId": 4096,
            "clips": [{
                "id": 7,
                "layer": 3,
                "startFrame": 10,
                "durationFrames": 100,
                "clipByUpperObject": true,
                "type": "video",
                "effects": [{
                    "id": "transform",
                    "enabled": true,
                    "known": true,
                    "params": {"x": 1.0, "scale": 125.0, "opacity": 0.75, "tint": "#80402010"},
                    "keyframes": {"x": [
                        {"frame": 0, "value": 0.0, "interp": "linear"},
                        {"frame": 100, "value": 100.0, "interp": "linear"}
                    ]}
                }, {
                    "id": "audio",
                    "enabled": true,
                    "known": false,
                    "params": {"playMode": "direct", "directTime": 3.0, "volume": 0.5, "limiter": true},
                    "keyframes": {}
                }]
            }]
        }))
        .expect("serialize scene fixture")
    }

    #[test]
    fn builds_scene_outputs_and_reuses_numeric_frame_cache() {
        let mut plan = BakePlan::parse(&scene_json()).expect("valid plan");
        assert_eq!(plan.effect_count, 2);
        let key = EvaluationKey {
            current_frame: 60,
            full_bake: true,
            prefetch_frames: 30,
        };
        let first = plan.compute(key);
        assert_eq!(first.renders.len(), 1);
        assert_eq!(first.audio.len(), 1);
        assert_eq!(first.renders[0].x, 50.0);
        assert_eq!(first.renders[0].scale_x, 1.25);
        assert_eq!(first.audio[0].direct_time, 3.0);
        assert_eq!(first.audio[0].volume, 0.5);
        assert_eq!(first.counts.numeric_batch_calls, 1);
        assert_eq!(first.counts.numeric_track_count, 1);
        let tint = first
            .params
            .iter()
            .find(|entry| entry.param_name.starts_with(b"tint"))
            .expect("tint parameter");
        assert_eq!(tint.param_type, PARAM_TYPE_COLOR);
        assert_eq!(
            tint.value,
            [64.0 / 255.0, 32.0 / 255.0, 16.0 / 255.0, 128.0 / 255.0]
        );

        let repeated = plan.compute(key);
        assert_eq!(repeated.counts.numeric_batch_calls, 0);
        assert_eq!(repeated.counts.numeric_track_count, 0);
    }

    #[test]
    fn typed_audio_plan_evaluates_plugin_parameters_and_preserves_host_metadata() {
        let input = json!({
            "version": 3,
            "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
            "scenes": [{"id": 1, "name": "Scene", "duration": 120}],
            "clips": [{
                "id": 7,
                "sceneId": 1,
                "type": "audio",
                "start": 10,
                "duration": 20,
                "layer": 0,
                "params": {},
                "audioPlugins": [{
                    "id": "CLAP:org.example.gain:0",
                    "enabled": true,
                    "params": {"0": 0.25, "1": true},
                    "keyframes": {"0": {
                        "start": {"frame": 0, "value": 0.25, "interp": "linear"},
                        "points": [{"frame": 10, "value": 0.75, "interp": "linear"}]
                    }},
                    "hostBackend": "truce-rack",
                    "nativeId": "org.example.gain",
                    "path": "/plugins/gain.clap"
                }],
                "effects": [{
                    "id": "audio",
                    "name": "Audio",
                    "enabled": true,
                    "params": {"path": "tone.wav"}
                }]
            }]
        });
        let bytes = serde_json::to_vec(&input).expect("serialize project fixture");
        let document = crate::project::parse_project_document(&bytes).expect("project parses");
        let mut plan = SceneRenderPlan::from_document(&document, 1).expect("scene plan builds");

        let frame = plan.evaluate(15);
        assert_eq!(frame.audio.len(), 1);
        assert_eq!(frame.audio[0].plugins.len(), 1);
        let plugin = &frame.audio[0].plugins[0];
        assert_eq!(plugin.id, "CLAP:org.example.gain:0");
        assert!(plugin.enabled);
        assert_eq!(plugin.params["0"], json!(0.5));
        assert_eq!(plugin.params["1"], json!(true));
        assert_eq!(plugin.extra["hostBackend"], json!("truce-rack"));
        assert_eq!(plugin.extra["nativeId"], json!("org.example.gain"));
        assert_eq!(plugin.extra["path"], json!("/plugins/gain.clap"));
    }

    #[test]
    fn typed_frame_renders_counter_and_simple_shape_objects() {
        let input = serde_json::to_vec(&json!({
            "fps": 60.0,
            "clips": [{
                "id": 1,
                "layer": 0,
                "durationFrames": 120,
                "type": "counter",
                "effects": [{
                    "id": "counter",
                    "known": true,
                    "params": {
                        "mode": "value",
                        "startValue": 10,
                        "endValue": 110,
                        "digits": 4,
                        "decimals": 1,
                        "prefix": "[",
                        "suffix": "]",
                        "fontSize": 64,
                        "opacity": 0.5
                    }
                }]
            }, {
                "id": 2,
                "layer": 1,
                "durationFrames": 120,
                "type": "polygon_shape",
                "effects": [{
                    "id": "polygon_shape",
                    "known": true,
                    "params": {"sizeW": 320, "sizeH": 240, "sides": 6, "rotation": 15}
                }]
            }, {
                "id": 3,
                "layer": 2,
                "durationFrames": 120,
                "type": "pie_shape",
                "effects": [{
                    "id": "pie_shape",
                    "known": true,
                    "params": {"sizeW": 400, "sizeH": 300, "angle": 123.5}
                }]
            }]
        }))
        .expect("serialize custom-object fixture");
        let mut plan = BakePlan::parse(&input).expect("valid custom-object plan");
        let frame = plan.compute_frame(EvaluationKey {
            current_frame: 30,
            full_bake: false,
            prefetch_frames: 0,
        });

        let counter = frame
            .layers
            .iter()
            .find(|layer| layer.clip_id == 1)
            .and_then(|layer| layer.text.as_ref())
            .expect("counter produces a text render plan");
        assert_eq!(counter.content, "[0035.0]");
        assert_eq!(counter.font_size, 64.0);
        assert_eq!(counter.opacity, 0.5);
        assert!(counter.outline_enabled);

        let polygon = frame
            .layers
            .iter()
            .find(|layer| layer.clip_id == 2)
            .and_then(|layer| layer.shape.as_ref())
            .expect("polygon object produces a shape render plan");
        assert_eq!(polygon.kind, ShapeKind::Polygon);
        assert_eq!(
            (polygon.width, polygon.height, polygon.sides),
            (320.0, 240.0, 6)
        );
        assert_eq!(polygon.rotation_degrees, 15.0);
        assert!(!polygon.normalize_vertices);
        assert!(!polygon.even_sides_half_step);
        assert_eq!(polygon.edge_padding, 4.0);

        let pie = frame
            .layers
            .iter()
            .find(|layer| layer.clip_id == 3)
            .and_then(|layer| layer.shape.as_ref())
            .expect("pie object produces a shape render plan");
        assert_eq!(pie.kind, ShapeKind::Pie);
        assert_eq!(pie.sweep_degrees, 123.5);
        assert_eq!((pie.width, pie.height), (400.0, 300.0));
    }

    #[test]
    fn counter_modes_and_padding_match_the_qml_contract() {
        let mut params = BTreeMap::from([
            ("digits".to_owned(), json!(3)),
            ("decimals".to_owned(), json!(2)),
            ("prefix".to_owned(), json!("T=")),
            ("suffix".to_owned(), json!("s")),
            ("mode".to_owned(), json!("time")),
        ]);
        assert_eq!(counter_text(&params, 75, 120, 60.0), "T=001.25s");

        params.insert("mode".to_owned(), json!("frame"));
        params.insert("decimals".to_owned(), json!(0));
        assert_eq!(counter_text(&params, 7, 120, 60.0), "T=007s");

        params.insert("mode".to_owned(), json!("value"));
        params.insert("startValue".to_owned(), json!(-10));
        params.insert("endValue".to_owned(), json!(10));
        params.insert("digits".to_owned(), json!(2));
        assert_eq!(counter_text(&params, 30, 120, 60.0), "T=-05s");
    }

    #[test]
    fn typed_frame_renders_the_remaining_canvas_objects() {
        let clips = [
            (4, "track_line", json!({"lineWidth": 12, "arrow": false})),
            (5, "star", json!({"count": 25, "speed": 2, "seed": 9})),
            (
                6,
                "radial_lines",
                json!({"lineCount": 32, "spinSpeed": 0.5, "seed": 3}),
            ),
            (7, "flare", json!({"radius": 90, "ghosts": 0})),
            (8, "lens_flare_object", json!({"radius": 180, "ghosts": 4})),
        ]
        .into_iter()
        .map(|(id, clip_type, params)| {
            json!({
                "id": id,
                "layer": id,
                "durationFrames": 120,
                "type": clip_type,
                "effects": [{"id": clip_type, "known": true, "params": params}]
            })
        })
        .collect::<Vec<_>>();
        let input = serde_json::to_vec(&json!({"fps": 60.0, "clips": clips}))
            .expect("serialize Canvas-object fixture");
        let mut plan = BakePlan::parse(&input).expect("valid Canvas-object plan");
        let frame = plan.compute_frame(EvaluationKey {
            current_frame: 10,
            full_bake: false,
            prefetch_frames: 0,
        });
        assert_eq!(frame.layers.len(), 5);

        let procedural = |clip_id| {
            frame
                .layers
                .iter()
                .find(|layer| layer.clip_id == clip_id)
                .and_then(|layer| layer.procedural.as_ref())
                .expect("clip produces a procedural render plan")
        };
        let ProceduralObjectRenderPlan::TrackLine(track) = procedural(4) else {
            panic!("track line plan kind");
        };
        assert_eq!(track.line_width, 12.0);
        assert!(!track.arrow);

        let ProceduralObjectRenderPlan::ParticleField(stars) = procedural(5) else {
            panic!("particle field plan kind");
        };
        assert_eq!((stars.count, stars.seed, stars.relative_frame), (25, 9, 10));

        let ProceduralObjectRenderPlan::RadialLines(lines) = procedural(6) else {
            panic!("radial lines plan kind");
        };
        assert_eq!(
            (lines.line_count, lines.seed, lines.relative_frame),
            (32, 3, 10)
        );

        let ProceduralObjectRenderPlan::LensFlare(flare) = procedural(7) else {
            panic!("flare plan kind");
        };
        assert_eq!((flare.radius, flare.ghosts), (90.0, 0));

        let ProceduralObjectRenderPlan::LensFlare(lens_flare) = procedural(8) else {
            panic!("lens flare plan kind");
        };
        assert_eq!((lens_flare.radius, lens_flare.ghosts), (180.0, 4));
    }

    #[test]
    fn group_controls_apply_to_the_same_following_layer_range_as_qml() {
        let input = serde_json::to_vec(&json!({
            "fps": 60.0,
            "clips": [{
                "id": 1,
                "layer": 1,
                "durationFrames": 120,
                "type": "GroupControl",
                "effects": [{
                    "id": "GroupControl",
                    "known": true,
                    "params": {
                        "layerCount": 1,
                        "x": 100,
                        "y": 20,
                        "z": 5,
                        "scale": 200,
                        "rotationZ": 90,
                        "opacity": 0.5
                    }
                }]
            }, {
                "id": 2,
                "layer": 2,
                "durationFrames": 120,
                "type": "rect",
                "effects": [{
                    "id": "transform",
                    "known": true,
                    "params": {"x": 10, "y": 0, "z": 2, "rotationZ": 5, "opacity": 0.8}
                }, {
                    "id": "rect",
                    "known": true,
                    "params": {"sizeW": 20, "sizeH": 20}
                }]
            }, {
                "id": 3,
                "layer": 3,
                "durationFrames": 120,
                "type": "rect",
                "effects": [{
                    "id": "transform",
                    "known": true,
                    "params": {"x": 10, "opacity": 0.8}
                }, {
                    "id": "rect",
                    "known": true,
                    "params": {"sizeW": 20, "sizeH": 20}
                }]
            }]
        }))
        .expect("serialize group-control fixture");
        let mut plan = BakePlan::parse(&input).expect("valid group-control plan");
        let frame = plan.compute_frame(EvaluationKey {
            current_frame: 0,
            full_bake: false,
            prefetch_frames: 0,
        });
        let controlled = frame
            .layers
            .iter()
            .find(|layer| layer.clip_id == 2)
            .expect("controlled layer");
        assert!((controlled.transform.x - 100.0).abs() < 0.001);
        assert!((controlled.transform.y - 40.0).abs() < 0.001);
        assert!((controlled.transform.z - 9.0).abs() < 0.001);
        assert_eq!(controlled.transform.rotation_z, 95.0);
        assert!((controlled.transform.opacity - 0.4).abs() < 0.001);

        let outside_range = frame
            .layers
            .iter()
            .find(|layer| layer.clip_id == 3)
            .expect("outside layer");
        assert_eq!(outside_range.transform.x, 10.0);
        assert_eq!(outside_range.transform.rotation_z, 0.0);
        assert!((outside_range.transform.opacity - 0.8).abs() < 0.001);
    }

    #[test]
    fn lowest_active_camera_control_owns_the_scene_camera() {
        let cameras = [(1, 5, 10), (2, 2, 30)]
            .into_iter()
            .map(|(id, layer, z)| {
                json!({
                    "id": id,
                    "layer": layer,
                    "durationFrames": 120,
                    "type": "camera_control",
                    "effects": [{
                        "id": "camera_control",
                        "known": true,
                        "params": {
                            "x": 10,
                            "y": 20,
                            "z": z,
                            "tx": -30,
                            "ty": -40,
                            "tz": 50,
                            "roll": 12,
                            "fov": 60
                        }
                    }]
                })
            })
            .collect::<Vec<_>>();
        let input = serde_json::to_vec(&json!({"fps": 60.0, "height": 720, "clips": cameras}))
            .expect("serialize camera-control fixture");
        let mut plan = BakePlan::parse(&input).expect("valid camera-control plan");
        let camera = plan
            .compute_frame(EvaluationKey {
                current_frame: 0,
                full_bake: false,
                prefetch_frames: 0,
            })
            .camera
            .expect("active camera control");
        assert_eq!((camera.position_x, camera.position_y), (10.0, -20.0));
        assert!((camera.position_z - 653.5383).abs() < 0.001);
        assert_eq!(
            (camera.target_x, camera.target_y, camera.target_z),
            (-30.0, 40.0, 50.0)
        );
        assert_eq!(camera.roll_degrees, 12.0);
        assert_eq!(camera.field_of_view_degrees, 60.0);
    }

    #[test]
    fn camera_control_parameters_fall_back_to_the_legacy_camera_effect() {
        let input = serde_json::to_vec(&json!({
            "fps": 60.0,
            "height": 720,
            "clips": [{
                "id": 1,
                "layer": 1,
                "durationFrames": 120,
                "type": "camera",
                "effects": [{
                    "id": "camera",
                    "known": true,
                    "params": {"x": 40, "y": 20, "fov": 60}
                }, {
                    "id": "camera_control",
                    "known": true,
                    "params": {"x": 10}
                }]
            }]
        }))
        .expect("serialize legacy camera fixture");
        let mut plan = BakePlan::parse(&input).expect("valid legacy camera plan");
        let camera = plan
            .compute_frame(EvaluationKey {
                current_frame: 0,
                full_bake: false,
                prefetch_frames: 0,
            })
            .camera
            .expect("legacy camera control");
        assert_eq!((camera.position_x, camera.position_y), (10.0, -20.0));
        assert_eq!(camera.field_of_view_degrees, 60.0);
    }

    #[test]
    fn on_demand_selection_uses_temporal_buckets() {
        let input = serde_json::to_vec(&json!({
            "fps": 60.0,
            "clips": (0..1000).map(|index| json!({
                "id": index,
                "startFrame": index * 240,
                "durationFrames": 30
            })).collect::<Vec<_>>()
        }))
        .expect("serialize bucket fixture");
        let mut plan = BakePlan::parse(&input).expect("valid plan");
        let result = plan.compute(EvaluationKey {
            current_frame: 500 * 240 + 5,
            full_bake: false,
            prefetch_frames: 10,
        });
        assert_eq!(result.counts.clips_visited, 1);
        assert_eq!(result.renders.len(), 1);
        assert_eq!(result.renders[0].clip_id, 500);
    }

    #[test]
    fn parameter_names_are_truncated_on_utf8_boundaries() {
        let entry = pack_parameter(1, 2, "参数参数参数参数参数参数", &Value::from(3.0));
        let length = entry
            .param_name
            .iter()
            .position(|byte| *byte == 0)
            .expect("parameter name remains terminated");
        assert!(length <= 19);
        assert!(std::str::from_utf8(&entry.param_name[..length]).is_ok());
        assert_eq!(entry.value[0], 3.0);
    }

    #[test]
    fn ffi_capacity_query_does_not_partially_write_outputs() {
        let input = scene_json();
        let mut handle = std::ptr::null_mut();
        assert_eq!(
            unsafe { aviqtl_timeline_bake_plan_create(input.as_ptr(), input.len(), &mut handle) },
            STATUS_OK
        );
        assert!(!handle.is_null());
        assert_eq!(unsafe { aviqtl_timeline_bake_plan_effect_count(handle) }, 2);

        let mut counts = AviQtlSceneBakeCounts::default();
        assert_eq!(
            unsafe {
                aviqtl_timeline_bake_plan_evaluate(
                    handle,
                    60,
                    1,
                    30,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    &mut counts,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(counts.render_count, 1);
        assert_eq!(counts.audio_count, 1);
        assert!(counts.param_count > 1);

        let sentinel = AviQtlEffectParamEntry {
            clip_id: u32::MAX,
            ..AviQtlEffectParamEntry::default()
        };
        let render_sentinel = AviQtlRenderBakeOutput {
            clip_id: -77,
            ..AviQtlRenderBakeOutput::default()
        };
        let audio_sentinel = AviQtlAudioBakeOutput {
            clip_id: -88,
            ..AviQtlAudioBakeOutput::default()
        };
        let mut renders = vec![render_sentinel; counts.render_count];
        let mut audio = vec![audio_sentinel; counts.audio_count];
        let mut params = vec![sentinel; counts.param_count - 1];
        assert_eq!(
            unsafe {
                aviqtl_timeline_bake_plan_evaluate(
                    handle,
                    60,
                    1,
                    30,
                    renders.as_mut_ptr(),
                    renders.len(),
                    audio.as_mut_ptr(),
                    audio.len(),
                    params.as_mut_ptr(),
                    params.len(),
                    &mut counts,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        assert!(renders.iter().all(|entry| entry.clip_id == -77));
        assert!(audio.iter().all(|entry| entry.clip_id == -88));
        assert!(params.iter().all(|entry| entry.clip_id == u32::MAX));

        params.resize(counts.param_count, AviQtlEffectParamEntry::default());
        assert_eq!(
            unsafe {
                aviqtl_timeline_bake_plan_evaluate(
                    handle,
                    60,
                    1,
                    30,
                    renders.as_mut_ptr(),
                    renders.len(),
                    audio.as_mut_ptr(),
                    audio.len(),
                    params.as_mut_ptr(),
                    params.len(),
                    &mut counts,
                )
            },
            STATUS_OK
        );
        assert_eq!(renders[0].clip_id, 7);
        assert_eq!(audio[0].clip_id, 7);
        unsafe { aviqtl_timeline_bake_plan_destroy(handle) };
    }

    #[test]
    fn reset_rejects_invalid_json_without_replacing_the_plan() {
        let input = scene_json();
        let mut handle = std::ptr::null_mut();
        assert_eq!(
            unsafe { aviqtl_timeline_bake_plan_create(input.as_ptr(), input.len(), &mut handle) },
            STATUS_OK
        );
        let invalid = b"{";
        assert_eq!(
            unsafe { aviqtl_timeline_bake_plan_reset(handle, invalid.as_ptr(), invalid.len()) },
            STATUS_INVALID_JSON
        );
        assert_eq!(unsafe { aviqtl_timeline_bake_plan_effect_count(handle) }, 2);
        unsafe { aviqtl_timeline_bake_plan_destroy(handle) };
    }

    #[test]
    fn ffi_rejects_overlapping_output_ranges() {
        let input = scene_json();
        let mut handle = std::ptr::null_mut();
        assert_eq!(
            unsafe { aviqtl_timeline_bake_plan_create(input.as_ptr(), input.len(), &mut handle) },
            STATUS_OK
        );
        let mut storage = std::mem::MaybeUninit::<AviQtlRenderBakeOutput>::uninit();
        let render = storage.as_mut_ptr();
        let counts = render.cast::<AviQtlSceneBakeCounts>();
        assert_eq!(
            unsafe {
                aviqtl_timeline_bake_plan_evaluate(
                    handle,
                    60,
                    1,
                    30,
                    render,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    counts,
                )
            },
            STATUS_OVERLAPPING_BUFFERS
        );
        unsafe { aviqtl_timeline_bake_plan_destroy(handle) };
    }
}
