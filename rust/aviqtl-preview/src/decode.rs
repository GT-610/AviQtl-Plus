use aviqtl_media::{VideoDecoder, VideoFrame, decode_image};
use aviqtl_render::{
    BlendMode, LayerCrop, LayerTransform, TextRasterizer, VisualEffect,
    rasterize_procedural_object, rasterize_shape,
};
use aviqtl_rust_core::api::{
    CameraRenderPlan, MediaPlaybackPlan, ProceduralObjectRenderPlan, ShapeRenderPlan,
    TextRenderPlan,
};
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

const MAX_NATIVE_CANVAS_CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecodeKind {
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreviewContent {
    Media {
        path: PathBuf,
        kind: DecodeKind,
        playback: MediaPlaybackPlan,
    },
    Shape {
        plan: ShapeRenderPlan,
        timestamp_seconds: f64,
    },
    Text {
        plan: TextRenderPlan,
    },
    Procedural {
        plan: ProceduralObjectRenderPlan,
        timestamp_seconds: f64,
    },
    NativeCanvas {
        width: u32,
        height: u32,
    },
    Scene {
        scene: Box<PreviewScene>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewScene {
    pub instance_key: u64,
    pub width: u32,
    pub height: u32,
    pub camera: Option<CameraRenderPlan>,
    pub opaque_background: bool,
    pub layers: Vec<PreviewSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewSource {
    pub clip_id: i32,
    pub content: PreviewContent,
    pub timeline_layer: i32,
    pub transform: LayerTransform,
    pub blend_mode: BlendMode,
    pub crop: LayerCrop,
    pub mask: Option<Box<PreviewScene>>,
    pub effects: Vec<VisualEffect>,
}

impl PreviewSource {
    pub fn render_cache_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.clip_id.hash(&mut hasher);
        match &self.content {
            PreviewContent::Media { path, kind, .. } => {
                path.hash(&mut hasher);
                kind.hash(&mut hasher);
            }
            PreviewContent::Shape { plan, .. } => {
                "rect".hash(&mut hasher);
                std::mem::discriminant(&plan.kind).hash(&mut hasher);
                plan.width.to_bits().hash(&mut hasher);
                plan.height.to_bits().hash(&mut hasher);
                plan.sides.hash(&mut hasher);
                plan.sweep_degrees.to_bits().hash(&mut hasher);
                plan.corner_radius.to_bits().hash(&mut hasher);
                plan.inner_radius_percent.to_bits().hash(&mut hasher);
                plan.rotation_degrees.to_bits().hash(&mut hasher);
                plan.normalize_vertices.hash(&mut hasher);
                plan.even_sides_half_step.hash(&mut hasher);
                plan.edge_padding.to_bits().hash(&mut hasher);
                hash_color(plan.fill_color, &mut hasher);
                plan.use_gradient.hash(&mut hasher);
                hash_color(plan.gradient_color, &mut hasher);
                std::mem::discriminant(&plan.gradient_kind).hash(&mut hasher);
                hash_color(plan.stroke_color, &mut hasher);
                plan.stroke_width.to_bits().hash(&mut hasher);
                plan.dash_length.to_bits().hash(&mut hasher);
                plan.dash_space.to_bits().hash(&mut hasher);
            }
            PreviewContent::Text { plan } => {
                "text".hash(&mut hasher);
                plan.content.hash(&mut hasher);
                plan.font_family.hash(&mut hasher);
                plan.font_size.to_bits().hash(&mut hasher);
                plan.bold.hash(&mut hasher);
                plan.italic.hash(&mut hasher);
                plan.letter_spacing.to_bits().hash(&mut hasher);
                plan.line_spacing.to_bits().hash(&mut hasher);
                std::mem::discriminant(&plan.alignment).hash(&mut hasher);
                hash_color(plan.color, &mut hasher);
                plan.outline_enabled.hash(&mut hasher);
                hash_color(plan.outline_color, &mut hasher);
                plan.outline_width.to_bits().hash(&mut hasher);
                plan.shadow_enabled.hash(&mut hasher);
                hash_color(plan.shadow_color, &mut hasher);
                plan.shadow_offset_x.to_bits().hash(&mut hasher);
                plan.shadow_offset_y.to_bits().hash(&mut hasher);
                plan.background_enabled.hash(&mut hasher);
                hash_color(plan.background_color, &mut hasher);
                plan.background_radius.to_bits().hash(&mut hasher);
                plan.background_padding_x.to_bits().hash(&mut hasher);
                plan.background_padding_y.to_bits().hash(&mut hasher);
            }
            PreviewContent::Procedural { plan, .. } => {
                "procedural".hash(&mut hasher);
                hash_procedural_plan(plan, &mut hasher);
            }
            PreviewContent::NativeCanvas { width, height } => {
                "native-canvas".hash(&mut hasher);
                width.hash(&mut hasher);
                height.hash(&mut hasher);
            }
            PreviewContent::Scene { scene } => {
                "scene".hash(&mut hasher);
                scene.instance_key.hash(&mut hasher);
            }
        }
        self.effects.hash(&mut hasher);
        hasher.finish()
    }

    fn label(&self) -> String {
        match &self.content {
            PreviewContent::Media { path, .. } => path.display().to_string(),
            PreviewContent::Shape { .. } => "procedural shape".to_owned(),
            PreviewContent::Text { .. } => "text object".to_owned(),
            PreviewContent::Procedural { plan, .. } => match plan {
                ProceduralObjectRenderPlan::TrackLine(_) => "track line object".to_owned(),
                ProceduralObjectRenderPlan::ParticleField(_) => "particle field object".to_owned(),
                ProceduralObjectRenderPlan::RadialLines(_) => "radial lines object".to_owned(),
                ProceduralObjectRenderPlan::LensFlare(_) => "lens flare object".to_owned(),
            },
            PreviewContent::NativeCanvas { .. } => "native package object".to_owned(),
            PreviewContent::Scene { scene } => format!("nested scene {}", scene.instance_key),
        }
    }
}

fn hash_color(color: aviqtl_rust_core::api::RgbaColor, hasher: &mut DefaultHasher) {
    color.red.hash(hasher);
    color.green.hash(hasher);
    color.blue.hash(hasher);
    color.alpha.hash(hasher);
}

fn hash_procedural_plan(plan: &ProceduralObjectRenderPlan, hasher: &mut DefaultHasher) {
    match plan {
        ProceduralObjectRenderPlan::TrackLine(plan) => {
            "track_line".hash(hasher);
            for value in [
                plan.width,
                plan.height,
                plan.start_x,
                plan.start_y,
                plan.end_x,
                plan.end_y,
                plan.line_width,
                plan.dash_length,
                plan.dash_space,
            ] {
                value.to_bits().hash(hasher);
            }
            plan.arrow.hash(hasher);
            hash_color(plan.color, hasher);
        }
        ProceduralObjectRenderPlan::ParticleField(plan) => {
            "particle_field".hash(hasher);
            for value in [
                plan.width,
                plan.height,
                plan.speed,
                plan.particle_size,
                plan.spread,
            ] {
                value.to_bits().hash(hasher);
            }
            plan.count.hash(hasher);
            plan.seed.hash(hasher);
            plan.relative_frame.hash(hasher);
            hash_color(plan.color, hasher);
        }
        ProceduralObjectRenderPlan::RadialLines(plan) => {
            "radial_lines".hash(hasher);
            for value in [
                plan.width,
                plan.height,
                plan.min_length,
                plan.max_length,
                plan.thickness,
                plan.randomness,
                plan.center_x,
                plan.center_y,
                plan.spin_speed,
            ] {
                value.to_bits().hash(hasher);
            }
            plan.line_count.hash(hasher);
            plan.seed.hash(hasher);
            plan.relative_frame.hash(hasher);
            hash_color(plan.color, hasher);
        }
        ProceduralObjectRenderPlan::LensFlare(plan) => {
            "lens_flare".hash(hasher);
            for value in [
                plan.width,
                plan.height,
                plan.center_x,
                plan.center_y,
                plan.radius,
                plan.strength,
            ] {
                value.to_bits().hash(hasher);
            }
            plan.ghosts.hash(hasher);
            hash_color(plan.color, hasher);
        }
    }
}

struct DecodeRequest {
    generation: u64,
    scene: PreviewScene,
}

struct DecodeQueue {
    state: Mutex<DecodeQueueState>,
    ready: Condvar,
}

struct DecodeQueueState {
    request: Option<DecodeRequest>,
    stopped: bool,
    reset: bool,
    image_budget: usize,
}

enum DecodeWork {
    Reset,
    Frame(DecodeRequest, usize),
}

impl DecodeQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(DecodeQueueState {
                request: None,
                stopped: false,
                reset: false,
                image_budget: 512 * 1024 * 1024,
            }),
            ready: Condvar::new(),
        }
    }

    fn submit(&self, request: DecodeRequest) {
        let mut state = self.state.lock().expect("decode queue lock");
        if !state.stopped {
            state.request = Some(request);
            self.ready.notify_one();
        }
    }

    fn take(&self) -> Option<DecodeWork> {
        let mut state = self.state.lock().expect("decode queue lock");
        loop {
            if state.stopped {
                return None;
            }
            if std::mem::take(&mut state.reset) {
                return Some(DecodeWork::Reset);
            }
            if let Some(request) = state.request.take() {
                return Some(DecodeWork::Frame(request, state.image_budget));
            }
            state = self.ready.wait(state).expect("decode queue wait");
        }
    }

    fn clear(&self) {
        let mut state = self.state.lock().expect("decode queue lock");
        state.request = None;
        state.reset = true;
        self.ready.notify_one();
    }

    fn set_image_budget(&self, bytes: usize) {
        let mut state = self.state.lock().expect("decode queue lock");
        if state.image_budget != bytes {
            state.image_budget = bytes;
            state.reset = true;
            self.ready.notify_one();
        }
    }

    fn stop(&self) {
        let mut state = self.state.lock().expect("decode queue lock");
        state.stopped = true;
        state.request = None;
        self.ready.notify_one();
    }
}

struct DecodeResult {
    generation: u64,
    batch: PreviewBatch,
}

pub enum DecodedContent {
    Frame(Arc<VideoFrame>),
    Scene(Box<DecodedScene>),
}

pub struct DecodedLayer {
    pub cache_key: u64,
    pub timeline_layer: i32,
    pub transform: LayerTransform,
    pub blend_mode: BlendMode,
    pub crop: LayerCrop,
    pub mask: Option<Box<DecodedScene>>,
    pub effects: Vec<VisualEffect>,
    pub content: DecodedContent,
}

pub struct DecodedScene {
    pub instance_key: u64,
    pub width: u32,
    pub height: u32,
    pub camera: Option<CameraRenderPlan>,
    pub opaque_background: bool,
    pub layers: Vec<DecodedLayer>,
}

pub fn frame_buffer_scene(
    instance_key: u64,
    width: u32,
    height: u32,
    camera: Option<CameraRenderPlan>,
    timeline_layer: i32,
    clear_below: bool,
    sources: &[PreviewSource],
) -> PreviewScene {
    PreviewScene {
        instance_key,
        width,
        height,
        camera,
        opaque_background: clear_below,
        layers: sources
            .iter()
            .filter(|source| source.timeline_layer < timeline_layer)
            .cloned()
            .map(|mut source| {
                source.mask = None;
                source
            })
            .collect(),
    }
}

pub fn upper_object_mask_scene(
    instance_key: u64,
    width: u32,
    height: u32,
    camera: Option<CameraRenderPlan>,
    timeline_layer: i32,
    sources: &[PreviewSource],
) -> Option<PreviewScene> {
    let mask_layer = sources
        .iter()
        .filter(|source| source.timeline_layer < timeline_layer)
        .map(|source| source.timeline_layer)
        .max()?;
    let mut source = sources
        .iter()
        .find(|source| source.timeline_layer == mask_layer)?
        .clone();
    source.mask = None;
    Some(PreviewScene {
        instance_key,
        width,
        height,
        camera,
        opaque_background: false,
        layers: vec![source],
    })
}

pub struct PreviewBatch {
    pub generation: u64,
    pub scene: DecodedScene,
    pub errors: Vec<String>,
}

pub struct MediaPreview {
    request_queue: Arc<DecodeQueue>,
    completed: Arc<Mutex<Option<DecodeResult>>>,
    worker: Option<JoinHandle<()>>,
    requested: Option<PreviewScene>,
    generation: u64,
    minimum_valid_generation: u64,
    displayed_generation: u64,
}

impl MediaPreview {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let request_queue = Arc::new(DecodeQueue::new());
        let completed = Arc::new(Mutex::new(None));
        let worker_completed = Arc::clone(&completed);
        let worker_queue = Arc::clone(&request_queue);
        let worker = thread::Builder::new()
            .name("aviqtl-media-preview".to_owned())
            .spawn(move || run_worker(worker_queue, worker_completed, wake))
            .expect("media preview worker must start");
        Self {
            request_queue,
            completed,
            worker: Some(worker),
            requested: None,
            generation: 0,
            minimum_valid_generation: 0,
            displayed_generation: 0,
        }
    }

    pub fn request(&mut self, scene: PreviewScene) {
        if self.requested.as_ref() == Some(&scene) {
            return;
        }
        self.request_fresh(scene);
    }

    pub fn request_fresh(&mut self, scene: PreviewScene) {
        self.requested = Some(scene.clone());
        self.generation = self.generation.wrapping_add(1);
        self.request_queue.submit(DecodeRequest {
            generation: self.generation,
            scene,
        });
    }

    pub fn poll(&mut self) -> Option<PreviewBatch> {
        let result = self
            .completed
            .lock()
            .expect("completed preview lock")
            .take()?;
        self.accepts_completed_generation(result.generation)
            .then_some(result.batch)
    }

    pub fn set_cache_size_mb(&mut self, megabytes: usize) {
        self.request_queue
            .set_image_budget(megabytes.saturating_mul(1024 * 1024));
    }

    fn accepts_completed_generation(&mut self, generation: u64) -> bool {
        if generation < self.minimum_valid_generation || generation <= self.displayed_generation {
            return false;
        }
        self.displayed_generation = generation;
        true
    }

    pub fn reset(&mut self) {
        self.requested = None;
        self.generation = self.generation.wrapping_add(1);
        self.minimum_valid_generation = self.generation;
        self.displayed_generation = self.generation;
        self.request_queue.clear();
        self.completed
            .lock()
            .expect("completed preview lock")
            .take();
    }
}

impl Drop for MediaPreview {
    fn drop(&mut self) {
        self.request_queue.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_worker(
    queue: Arc<DecodeQueue>,
    completed: Arc<Mutex<Option<DecodeResult>>>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let mut videos = HashMap::<PathBuf, VideoDecoder>::new();
    let mut images = ImageCache::default();
    let mut native_canvases = HashMap::<(u32, u32), Arc<VideoFrame>>::new();
    let text = TextRasterizer::new();
    while let Some(work) = queue.take() {
        let (request, image_budget) = match work {
            DecodeWork::Reset => {
                videos.clear();
                images.clear();
                native_canvases.clear();
                continue;
            }
            DecodeWork::Frame(request, budget) => (request, budget),
        };
        let mut active_videos = HashSet::new();
        collect_video_paths(&request.scene, &mut active_videos);
        videos.retain(|path, _| active_videos.contains(path));
        images.set_budget(image_budget);
        let mut errors = Vec::new();
        let scene = decode_scene(
            request.scene,
            &mut videos,
            &mut images,
            &mut native_canvases,
            &text,
            &mut errors,
        );
        let batch = PreviewBatch {
            generation: request.generation,
            scene,
            errors,
        };
        *completed.lock().expect("completed preview lock") = Some(DecodeResult {
            generation: request.generation,
            batch,
        });
        (wake)();
    }
}

fn collect_video_paths(scene: &PreviewScene, paths: &mut HashSet<PathBuf>) {
    for source in &scene.layers {
        match &source.content {
            PreviewContent::Media {
                path,
                kind: DecodeKind::Video,
                ..
            } => {
                paths.insert(path.clone());
            }
            PreviewContent::Scene { scene } => collect_video_paths(scene, paths),
            _ => {}
        }
        if let Some(mask) = &source.mask {
            collect_video_paths(mask, paths);
        }
    }
}

#[derive(Default)]
struct ImageCache {
    frames: HashMap<PathBuf, (Arc<VideoFrame>, u64)>,
    bytes: usize,
    budget: usize,
    access: u64,
}

impl ImageCache {
    fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
        self.access = 0;
    }

    fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
        self.evict_to(budget);
    }

    fn evict_to(&mut self, bytes: usize) {
        while self.bytes > bytes {
            let Some(path) = self
                .frames
                .iter()
                .min_by_key(|(_, (_, access))| *access)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            if let Some((frame, _)) = self.frames.remove(&path) {
                self.bytes -= frame.rgba.len();
            }
        }
    }

    fn get(&mut self, path: &PathBuf) -> Option<Arc<VideoFrame>> {
        let (frame, access) = self.frames.get_mut(path)?;
        self.access = self.access.saturating_add(1);
        *access = self.access;
        Some(Arc::clone(frame))
    }

    fn insert(&mut self, path: PathBuf, frame: Arc<VideoFrame>) {
        let bytes = frame.rgba.len();
        if bytes > self.budget {
            return;
        }
        if let Some((old, _)) = self.frames.remove(&path) {
            self.bytes -= old.rgba.len();
        }
        self.evict_to(self.budget - bytes);
        self.access = self.access.saturating_add(1);
        self.frames.insert(path, (frame, self.access));
        self.bytes += bytes;
    }
}

fn decode_scene(
    scene: PreviewScene,
    videos: &mut HashMap<PathBuf, VideoDecoder>,
    images: &mut ImageCache,
    native_canvases: &mut HashMap<(u32, u32), Arc<VideoFrame>>,
    text: &TextRasterizer,
    errors: &mut Vec<String>,
) -> DecodedScene {
    let mut layers = Vec::with_capacity(scene.layers.len());
    for source in scene.layers {
        let cache_key = source.render_cache_key();
        let label = source.label();
        let PreviewSource {
            clip_id,
            content,
            timeline_layer,
            transform,
            blend_mode,
            crop,
            mask,
            effects,
        } = source;
        let decoded = match content {
            PreviewContent::Scene { scene } => DecodedContent::Scene(Box::new(decode_scene(
                *scene,
                videos,
                images,
                native_canvases,
                text,
                errors,
            ))),
            content => match decode_source(&content, videos, images, native_canvases, text) {
                Ok(frame) => DecodedContent::Frame(frame),
                Err(error) => {
                    errors.push(format!("clip #{clip_id} ({label}): {error}"));
                    continue;
                }
            },
        };
        layers.push(DecodedLayer {
            cache_key,
            timeline_layer,
            transform,
            blend_mode,
            crop,
            mask: mask.map(|scene| {
                Box::new(decode_scene(
                    *scene,
                    videos,
                    images,
                    native_canvases,
                    text,
                    errors,
                ))
            }),
            effects,
            content: decoded,
        });
    }
    DecodedScene {
        instance_key: scene.instance_key,
        width: scene.width,
        height: scene.height,
        camera: scene.camera,
        opaque_background: scene.opaque_background,
        layers,
    }
}

fn decode_source(
    content: &PreviewContent,
    videos: &mut HashMap<PathBuf, VideoDecoder>,
    images: &mut ImageCache,
    native_canvases: &mut HashMap<(u32, u32), Arc<VideoFrame>>,
    text: &TextRasterizer,
) -> Result<Arc<VideoFrame>, String> {
    match content {
        PreviewContent::Media {
            path,
            kind: DecodeKind::Image,
            ..
        } => {
            if let Some(frame) = images.get(path) {
                return Ok(frame);
            }
            let frame = Arc::new(decode_image(path).map_err(|error| error.to_string())?);
            images.insert(path.clone(), Arc::clone(&frame));
            Ok(frame)
        }
        PreviewContent::Media {
            path,
            kind: DecodeKind::Video,
            playback,
        } => {
            if !videos.contains_key(path) {
                let decoder = VideoDecoder::open(path).map_err(|error| error.to_string())?;
                videos.insert(path.clone(), decoder);
            }
            let decoder = videos
                .get_mut(path)
                .expect("video decoder was inserted above");
            let timestamp_seconds = playback.timestamp_seconds(decoder.source_fps());
            Ok(Arc::new(
                decoder
                    .decode_at(timestamp_seconds)
                    .map_err(|error| error.to_string())?,
            ))
        }
        PreviewContent::Shape {
            plan,
            timestamp_seconds,
        } => Ok(Arc::new(
            rasterize_shape(plan, *timestamp_seconds).map_err(|error| error.to_string())?,
        )),
        PreviewContent::Text { plan } => Ok(Arc::new(
            text.rasterize(plan, 0.0)
                .map_err(|error| error.to_string())?,
        )),
        PreviewContent::Procedural {
            plan,
            timestamp_seconds,
        } => Ok(Arc::new(
            rasterize_procedural_object(plan, *timestamp_seconds)
                .map_err(|error| error.to_string())?,
        )),
        PreviewContent::NativeCanvas { width, height } => {
            let key = (*width, *height);
            if let Some(frame) = native_canvases.get(&key) {
                return Ok(Arc::clone(frame));
            }
            let pixel_count = usize::try_from(*width)
                .ok()
                .and_then(|width| {
                    usize::try_from(*height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| "native canvas dimensions are too large".to_owned())?;
            if pixel_count > 512 * 1024 * 1024 {
                return Err("native canvas dimensions are too large".to_owned());
            }
            let frame = Arc::new(VideoFrame {
                width: *width,
                height: *height,
                rgba: vec![0; pixel_count],
                timestamp_seconds: 0.0,
            });
            if pixel_count <= MAX_NATIVE_CANVAS_CACHE_BYTES {
                let cached_bytes = native_canvases
                    .values()
                    .map(|cached| cached.rgba.len())
                    .sum::<usize>();
                if cached_bytes.saturating_add(pixel_count) > MAX_NATIVE_CANVAS_CACHE_BYTES {
                    native_canvases.clear();
                }
                native_canvases.insert(key, Arc::clone(&frame));
            }
            Ok(frame)
        }
        PreviewContent::Scene { .. } => unreachable!("nested scenes are decoded recursively"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::{
        RgbaColor, ShapeGradientKind, ShapeKind, TextAlignment, TextRenderPlan,
    };

    fn empty_scene() -> PreviewScene {
        PreviewScene {
            instance_key: 1,
            width: 1,
            height: 1,
            camera: None,
            opaque_background: false,
            layers: Vec::new(),
        }
    }

    #[test]
    fn stalled_consumer_retains_only_the_latest_completed_frame_and_export_can_repeat() {
        let (ready, received) = std::sync::mpsc::channel();
        let mut preview = MediaPreview::new(Arc::new(move || {
            let _ = ready.send(());
        }));
        let mut scene = empty_scene();
        scene.layers.push(shape_source(10));
        preview.request_fresh(scene.clone());
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let old_frame = {
            let completed = preview.completed.lock().unwrap();
            let DecodedContent::Frame(frame) =
                &completed.as_ref().unwrap().batch.scene.layers[0].content
            else {
                panic!("expected frame")
            };
            Arc::downgrade(frame)
        };
        for _ in 0..8 {
            preview.request_fresh(scene.clone());
            received
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
        assert!(old_frame.upgrade().is_none());
        assert_eq!(preview.poll().unwrap().generation, preview.generation);
        assert!(preview.poll().is_none());
        for _ in 0..8 {
            preview.request_fresh(scene.clone());
            received
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            assert_eq!(preview.poll().unwrap().generation, preview.generation);
        }
        preview.reset();
        assert!(preview.poll().is_none());
    }

    #[test]
    fn reset_and_budget_changes_reach_an_idle_worker_before_the_next_frame() {
        let queue = DecodeQueue::new();
        queue.submit(DecodeRequest {
            generation: 1,
            scene: empty_scene(),
        });
        queue.clear();
        queue.submit(DecodeRequest {
            generation: 2,
            scene: empty_scene(),
        });
        assert!(matches!(queue.take(), Some(DecodeWork::Reset)));
        assert!(matches!(
            queue.take(),
            Some(DecodeWork::Frame(DecodeRequest { generation: 2, .. }, _))
        ));
        queue.set_image_budget(1234);
        assert!(matches!(queue.take(), Some(DecodeWork::Reset)));
        queue.submit(DecodeRequest {
            generation: 3,
            scene: empty_scene(),
        });
        assert!(matches!(queue.take(), Some(DecodeWork::Frame(_, 1234))));
        queue.stop();
        assert!(queue.take().is_none());
    }

    #[test]
    fn image_cache_evicts_lru_entries_and_never_keeps_oversized_frames() {
        let frame = |bytes: u32| {
            Arc::new(VideoFrame {
                width: bytes / 4,
                height: 1,
                rgba: vec![0; bytes as usize],
                timestamp_seconds: 0.0,
            })
        };
        let mut cache = ImageCache::default();
        cache.set_budget(8);
        cache.insert("a".into(), frame(4));
        cache.insert("b".into(), frame(4));
        assert!(cache.get(&"a".into()).is_some());
        cache.insert("c".into(), frame(4));
        assert!(cache.get(&"b".into()).is_none());
        assert!(cache.get(&"a".into()).is_some());
        let oversized = frame(12);
        cache.insert("large".into(), Arc::clone(&oversized));
        assert_eq!(oversized.rgba.len(), 12);
        assert!(!cache.frames.contains_key(&PathBuf::from("large")));
        assert_eq!(cache.bytes, 8);
        cache.set_budget(4);
        assert_eq!(cache.bytes, 4);
        cache.clear();
        assert_eq!(cache.bytes, 0);
        assert!(cache.frames.is_empty());
    }

    #[test]
    fn active_video_paths_include_nested_scenes_and_masks() {
        use aviqtl_rust_core::api::{SceneRenderPlan, TimelineState};
        let document = TimelineState::from_json(br#"{
            "version":3,"settings":{"width":64,"height":64,"fps":30,"sampleRate":48000},
            "scenes":[{"id":1,"name":"Root","duration":30}],
            "clips":[{"id":1,"sceneId":1,"type":"video","start":0,"duration":30,"layer":0,
            "effects":[{"id":"video","name":"Video","enabled":true,"params":{"path":"source.mkv"}}]}]
        }"#).unwrap().snapshot();
        let playback = SceneRenderPlan::from_document(&document, 1)
            .unwrap()
            .evaluate(0)
            .layers[0]
            .media
            .clone()
            .unwrap();
        let mut source = shape_source(10);
        source.content = PreviewContent::Media {
            path: "nested.mkv".into(),
            kind: DecodeKind::Video,
            playback: playback.clone(),
        };
        let mut nested = empty_scene();
        nested.layers.push(source.clone());
        let mut mask = empty_scene();
        source.content = PreviewContent::Media {
            path: "mask.mkv".into(),
            kind: DecodeKind::Video,
            playback,
        };
        mask.layers.push(source);
        let mut root_source = shape_source(20);
        root_source.content = PreviewContent::Scene {
            scene: Box::new(nested),
        };
        root_source.mask = Some(Box::new(mask));
        let mut root = empty_scene();
        root.layers.push(root_source);
        let mut paths = HashSet::new();
        collect_video_paths(&root, &mut paths);
        assert_eq!(
            paths,
            HashSet::from([PathBuf::from("nested.mkv"), PathBuf::from("mask.mkv")])
        );
    }

    fn shape_source(red: u8) -> PreviewSource {
        PreviewSource {
            clip_id: 4,
            content: PreviewContent::Shape {
                plan: ShapeRenderPlan {
                    kind: ShapeKind::Polygon,
                    width: 200.0,
                    height: 100.0,
                    sides: 4,
                    sweep_degrees: 4.0,
                    corner_radius: 0.0,
                    inner_radius_percent: 50.0,
                    rotation_degrees: 0.0,
                    normalize_vertices: true,
                    even_sides_half_step: true,
                    edge_padding: 2.0,
                    fill_color: RgbaColor {
                        red,
                        green: 30,
                        blue: 40,
                        alpha: 255,
                    },
                    use_gradient: false,
                    gradient_color: RgbaColor {
                        red: 255,
                        green: 255,
                        blue: 255,
                        alpha: 255,
                    },
                    gradient_kind: ShapeGradientKind::Linear,
                    stroke_color: RgbaColor {
                        red: 255,
                        green: 255,
                        blue: 255,
                        alpha: 255,
                    },
                    stroke_width: 0.0,
                    dash_length: 0.0,
                    dash_space: 0.0,
                    opacity: 1.0,
                },
                timestamp_seconds: 0.0,
            },
            timeline_layer: 0,
            transform: LayerTransform::default(),
            blend_mode: BlendMode::Normal,
            crop: LayerCrop::default(),
            mask: None,
            effects: Vec::new(),
        }
    }

    #[test]
    fn shape_edits_invalidate_the_gpu_texture_at_the_same_frame() {
        assert_ne!(
            shape_source(10).render_cache_key(),
            shape_source(20).render_cache_key()
        );
    }

    #[test]
    fn export_requests_can_repeat_an_identical_static_scene() {
        let mut preview = MediaPreview::new(Arc::new(|| {}));
        let scene = PreviewScene {
            instance_key: 1,
            width: 64,
            height: 64,
            camera: None,
            opaque_background: false,
            layers: Vec::new(),
        };

        preview.request(scene.clone());
        let first_generation = preview.generation;
        preview.request(scene.clone());
        assert_eq!(preview.generation, first_generation);
        preview.request_fresh(scene);
        assert_eq!(preview.generation, first_generation.wrapping_add(1));
    }

    #[test]
    fn decode_queue_keeps_only_the_newest_pending_request() {
        let queue = DecodeQueue::new();
        queue.submit(DecodeRequest {
            generation: 1,
            scene: PreviewScene {
                instance_key: 1,
                width: 1,
                height: 1,
                camera: None,
                opaque_background: false,
                layers: Vec::new(),
            },
        });
        queue.submit(DecodeRequest {
            generation: 2,
            scene: PreviewScene {
                instance_key: 2,
                width: 1,
                height: 1,
                camera: None,
                opaque_background: false,
                layers: Vec::new(),
            },
        });

        let DecodeWork::Frame(request, _) = queue.take().expect("latest request is available")
        else {
            panic!("expected frame");
        };
        assert_eq!(request.generation, 2);
        assert_eq!(request.scene.instance_key, 2);
    }

    #[test]
    fn playback_accepts_the_latest_completed_frame_without_waiting_for_the_latest_request() {
        let mut preview = MediaPreview::new(Arc::new(|| {}));
        let scene = PreviewScene {
            instance_key: 1,
            width: 1,
            height: 1,
            camera: None,
            opaque_background: false,
            layers: Vec::new(),
        };

        preview.request_fresh(scene.clone());
        let first = preview.generation;
        preview.request_fresh(scene.clone());
        preview.request_fresh(scene);

        assert!(first < preview.generation);
        assert!(preview.accepts_completed_generation(first));
        assert!(!preview.accepts_completed_generation(first));
        assert!(preview.accepts_completed_generation(preview.generation));
    }

    #[test]
    fn reset_rejects_results_from_the_previous_preview_source() {
        let mut preview = MediaPreview::new(Arc::new(|| {}));
        let scene = PreviewScene {
            instance_key: 1,
            width: 1,
            height: 1,
            camera: None,
            opaque_background: false,
            layers: Vec::new(),
        };

        preview.request_fresh(scene);
        let stale = preview.generation;
        preview.reset();

        assert!(!preview.accepts_completed_generation(stale));
    }

    fn text_source(content: &str) -> PreviewSource {
        PreviewSource {
            clip_id: 5,
            content: PreviewContent::Text {
                plan: TextRenderPlan {
                    content: content.to_owned(),
                    font_family: "sans-serif".to_owned(),
                    font_size: 48.0,
                    bold: false,
                    italic: false,
                    letter_spacing: 0.0,
                    line_spacing: 0.0,
                    alignment: TextAlignment::Center,
                    color: RgbaColor {
                        red: 255,
                        green: 255,
                        blue: 255,
                        alpha: 255,
                    },
                    outline_enabled: false,
                    outline_color: RgbaColor {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: 255,
                    },
                    outline_width: 2.0,
                    shadow_enabled: false,
                    shadow_color: RgbaColor {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: 128,
                    },
                    shadow_offset_x: 5.0,
                    shadow_offset_y: 5.0,
                    background_enabled: false,
                    background_color: RgbaColor {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: 128,
                    },
                    background_radius: 10.0,
                    background_padding_x: 20.0,
                    background_padding_y: 10.0,
                    opacity: 1.0,
                },
            },
            timeline_layer: 0,
            transform: LayerTransform::default(),
            blend_mode: BlendMode::Normal,
            crop: LayerCrop::default(),
            mask: None,
            effects: Vec::new(),
        }
    }

    #[test]
    fn text_edits_invalidate_the_gpu_texture_at_the_same_frame() {
        assert_ne!(
            text_source("before").render_cache_key(),
            text_source("after").render_cache_key()
        );
    }

    #[test]
    fn nested_preview_decodes_leaf_layers_into_the_scene_tree() {
        let root = PreviewScene {
            instance_key: 11,
            width: 1280,
            height: 720,
            camera: None,
            opaque_background: false,
            layers: vec![PreviewSource {
                clip_id: 9,
                content: PreviewContent::Scene {
                    scene: Box::new(PreviewScene {
                        instance_key: 22,
                        width: 640,
                        height: 360,
                        camera: None,
                        opaque_background: false,
                        layers: vec![shape_source(10)],
                    }),
                },
                timeline_layer: 3,
                transform: LayerTransform::default(),
                blend_mode: BlendMode::Normal,
                crop: LayerCrop::default(),
                mask: None,
                effects: Vec::new(),
            }],
        };
        let mut errors = Vec::new();
        let decoded = decode_scene(
            root,
            &mut HashMap::new(),
            &mut ImageCache::default(),
            &mut HashMap::new(),
            &TextRasterizer::new(),
            &mut errors,
        );
        assert!(errors.is_empty());
        assert_eq!(decoded.instance_key, 11);
        assert_eq!(decoded.layers.len(), 1);
        let DecodedContent::Scene(nested) = &decoded.layers[0].content else {
            panic!("root layer should remain a nested scene");
        };
        assert_eq!(nested.instance_key, 22);
        assert_eq!(nested.layers.len(), 1);
        assert!(matches!(nested.layers[0].content, DecodedContent::Frame(_)));
    }

    #[test]
    fn native_canvas_frames_are_cached_by_dimensions() {
        let content = PreviewContent::NativeCanvas {
            width: 64,
            height: 32,
        };
        let mut videos = HashMap::new();
        let mut images = ImageCache::default();
        let mut native_canvases = HashMap::new();
        let text = TextRasterizer::new();
        let first = decode_source(
            &content,
            &mut videos,
            &mut images,
            &mut native_canvases,
            &text,
        )
        .expect("native canvas decodes");
        let second = decode_source(
            &content,
            &mut videos,
            &mut images,
            &mut native_canvases,
            &text,
        )
        .expect("native canvas reuses its frame");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.rgba.len(), 64 * 32 * 4);

        let different = decode_source(
            &PreviewContent::NativeCanvas {
                width: 32,
                height: 64,
            },
            &mut videos,
            &mut images,
            &mut native_canvases,
            &text,
        )
        .expect("different native canvas decodes");
        assert!(!Arc::ptr_eq(&first, &different));

        for width in 1..=32 {
            decode_source(
                &PreviewContent::NativeCanvas { width, height: 1 },
                &mut videos,
                &mut images,
                &mut native_canvases,
                &text,
            )
            .expect("native canvas dimension change decodes");
        }
        let cached_bytes = native_canvases
            .values()
            .map(|cached| cached.rgba.len())
            .sum::<usize>();
        assert!(cached_bytes <= MAX_NATIVE_CANVAS_CACHE_BYTES);
    }

    #[test]
    fn frame_buffer_captures_only_strictly_smaller_qt_layers() {
        let mut front = shape_source(10);
        front.timeline_layer = 1;
        let mut same = shape_source(20);
        same.timeline_layer = 3;
        let mut behind = shape_source(30);
        behind.timeline_layer = 5;
        let scene = frame_buffer_scene(
            44,
            1920,
            1080,
            None,
            3,
            true,
            &[front.clone(), same, behind],
        );
        assert_eq!(scene.instance_key, 44);
        assert!(scene.opaque_background);
        assert_eq!(scene.layers, vec![front]);
    }

    #[test]
    fn upper_object_mask_uses_the_first_source_on_the_nearest_smaller_layer() {
        let mut layer_one = shape_source(10);
        layer_one.timeline_layer = 1;
        let mut first_layer_two = shape_source(20);
        first_layer_two.timeline_layer = 2;
        let mut second_layer_two = shape_source(30);
        second_layer_two.timeline_layer = 2;
        let scene = upper_object_mask_scene(
            55,
            1920,
            1080,
            None,
            4,
            &[layer_one, first_layer_two.clone(), second_layer_two],
        )
        .expect("nearest upper mask");
        assert_eq!(scene.layers, vec![first_layer_two]);
        assert!(!scene.opaque_background);
    }
}
