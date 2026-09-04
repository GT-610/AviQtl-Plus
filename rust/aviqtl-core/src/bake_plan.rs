use crate::abi::{
    AviQtlAudioBakeInput, AviQtlAudioBakeOutput, AviQtlEffectParamEntry, AviQtlRenderBakeInput,
    AviQtlRenderBakeOutput, AviQtlSceneBakeCounts, STATUS_BUFFER_TOO_SMALL,
    STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK, STATUS_OVERLAPPING_BUFFERS,
    ranges_overlap, slice_is_valid,
};
use crate::keyframe_document::{evaluate_resolved_track, parse_hex_color, resolve_track};
use crate::policy::{PlaybackMode, playback_mode};
use crate::timeline::{bake_audio, bake_render};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const FRAME_BUCKET_SIZE: i32 = 120;
const DEFAULT_SPEED_PERCENT: f32 = 100.0;
const PARAM_TYPE_FLOAT: u8 = 0;
const PARAM_TYPE_COLOR: u8 = 4;

fn default_max_clip_id() -> i32 {
    4096
}

fn default_enabled() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SceneInput {
    fps: f64,
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
        let points = resolve_track(&track, &fallback, duration);
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
    effects: Vec<CachedEffect>,
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
}

struct BakePlan {
    fps: f64,
    clips: Vec<CachedClip>,
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
        let mut clips = Vec::new();
        let mut time_buckets: HashMap<i32, Vec<usize>> = HashMap::new();
        let mut effect_count = 0_usize;
        for clip in input.clips {
            if clip.id < 0 || clip.id >= input.max_clip_id {
                continue;
            }
            effect_count = effect_count.saturating_add(clip.effects.len());
            let cached_index = clips.len();
            let cached = CachedClip::new(clip);
            let first_bucket = cached.start_frame / FRAME_BUCKET_SIZE;
            let last_bucket = cached.end_frame.max(cached.start_frame) / FRAME_BUCKET_SIZE;
            for bucket in first_bucket..=last_bucket {
                time_buckets.entry(bucket).or_default().push(cached_index);
            }
            clips.push(cached);
        }
        Ok(Self {
            fps: input.fps,
            clips,
            time_buckets,
            effect_count,
            pending: None,
        })
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
        let (selected, clips_visited) = self.selected_clips(key);
        let mut counts = AviQtlSceneBakeCounts {
            clips_visited,
            ..AviQtlSceneBakeCounts::default()
        };
        let mut renders = Vec::with_capacity(selected.len());
        let mut audio = Vec::new();
        let mut params = Vec::new();

        for index in selected {
            let clip = &mut self.clips[index];
            counts.selected_effect_count = counts
                .selected_effect_count
                .saturating_add(clip.effects.len() as u64);
            let relative_frame = key.current_frame.saturating_sub(clip.start_frame).max(0);
            for effect in &mut clip.effects {
                effect.prepare_numeric(relative_frame, &mut counts);
            }

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
                audio.push(output);
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
                for name in &effect.all_keys {
                    params.push(pack_parameter(
                        clip.clip_id,
                        effect_index as u8,
                        name,
                        &effect.value(name, relative_frame),
                    ));
                }
            }
            renders.push(bake_render(render_input));
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
        }
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
