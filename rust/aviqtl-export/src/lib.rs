#![forbid(unsafe_code)]

use aviqtl_audio::{TimelineAudioMixer, TimelineAudioSource};
use aviqtl_media::{
    StillImageFormat, VideoEncoder, VideoEncoderConfig, VideoFrame,
    available_audio_encoders as media_audio_encoders,
    available_video_encoders as media_video_encoders, save_still_image,
};
use aviqtl_preview::{MediaPreview, PreviewScene, PreviewSurface};
use aviqtl_rust_core::api::{
    AudioLayerPlan, ExportConfigurationError, ImageSequenceExportPlan, ImageSequenceExportRequest,
    VideoExportPlan, VideoExportRequest, export_codec_fallback, plan_export_audio_frame,
    plan_export_progress, plan_image_sequence_export, plan_video_export,
};
pub use aviqtl_rust_core::api::{ExportImageFormat, ExportProgressPlan};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportProject {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: i32,
    pub sample_rate: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    Video,
    ImageSequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub mode: ExportMode,
    pub output_path: PathBuf,
    pub image_format: ExportImageFormat,
    pub video_codec: String,
    pub crf: Option<i32>,
    pub bitrate_mbps: i32,
    pub preset: String,
    pub profile: String,
    pub audio_codec: String,
    pub audio_bitrate_kbps: i32,
    pub full_range: bool,
    pub start_frame: i32,
    pub end_frame: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportRuntimeSettings {
    pub image_quality: u8,
    pub sequence_padding: i32,
    pub progress_interval: i32,
    pub max_plugin_block_size: usize,
}

impl Default for ExportRuntimeSettings {
    fn default() -> Self {
        Self {
            image_quality: 95,
            sequence_padding: 6,
            progress_interval: 5,
            max_plugin_block_size: 1_024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportFrameRequest {
    pub project_instance_id: u64,
    pub frame: i32,
}

#[derive(Debug, Clone)]
pub enum ExportEvent {
    Progress(ExportProgressPlan),
    Finished(ExportResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportStartError {
    AlreadyRunning,
    Configuration(ExportConfigurationError),
    Worker(String),
}

impl fmt::Display for ExportStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => formatter.write_str("Export is already running"),
            Self::Configuration(error) => formatter.write_str(configuration_error_message(*error)),
            Self::Worker(error) => write!(formatter, "Export worker failed to start: {error}"),
        }
    }
}

impl std::error::Error for ExportStartError {}

impl From<ExportConfigurationError> for ExportStartError {
    fn from(error: ExportConfigurationError) -> Self {
        Self::Configuration(error)
    }
}

#[derive(Debug, Clone)]
enum ExportJob {
    Video {
        plan: VideoExportPlan,
        config: VideoEncoderConfig,
        fps: f64,
        max_plugin_block_size: usize,
        progress_interval: i32,
    },
    ImageSequence {
        plan: ImageSequenceExportPlan,
        output_directory: PathBuf,
        quality: u8,
        progress_interval: i32,
    },
}

impl ExportJob {
    fn start_frame(&self) -> i32 {
        match self {
            Self::Video { plan, .. } => plan.start_frame,
            Self::ImageSequence { plan, .. } => plan.start_frame,
        }
    }

    fn end_frame(&self) -> i32 {
        match self {
            Self::Video { plan, .. } => plan.end_frame,
            Self::ImageSequence { plan, .. } => plan.end_frame,
        }
    }

    fn total_frames(&self) -> i32 {
        match self {
            Self::Video { plan, .. } => plan.total_frames,
            Self::ImageSequence { plan, .. } => plan.total_frames,
        }
    }
}

struct ExportFrame {
    timeline_frame: i32,
    frame: VideoFrame,
    audio_sources: Vec<TimelineAudioSource>,
}

enum WorkerCommand {
    Frame(ExportFrame),
    Cancel,
}

enum WorkerEvent {
    FrameProcessed {
        done: i32,
        progress: Option<ExportProgressPlan>,
    },
    Finished {
        success: bool,
        message: String,
    },
}

struct ActiveExport {
    project_instance_id: u64,
    job: ExportJob,
    next_frame: i32,
    ready_for_frame: bool,
    awaiting_decode: bool,
    pending_audio: Option<Vec<TimelineAudioSource>>,
    commands: Sender<WorkerCommand>,
    events: Receiver<WorkerEvent>,
    worker: Option<JoinHandle<()>>,
    failure_override: Option<String>,
}

pub struct ExportManager {
    media_preview: MediaPreview,
    preview: PreviewSurface,
    active: Option<ActiveExport>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl ExportManager {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            media_preview: MediaPreview::new(wake.clone()),
            preview: PreviewSurface::new(device, queue),
            active: None,
            wake,
        }
    }

    pub fn is_exporting(&self) -> bool {
        self.active.is_some()
    }

    pub fn start(
        &mut self,
        project_instance_id: u64,
        project: ExportProject,
        request: ExportRequest,
        settings: ExportRuntimeSettings,
    ) -> Result<i32, ExportStartError> {
        if self.active.is_some() {
            return Err(ExportStartError::AlreadyRunning);
        }
        let job = build_job(project, request, settings)?;
        let total_frames = job.total_frames();
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let worker_job = job.clone();
        let wake = self.wake.clone();
        let worker = thread::Builder::new()
            .name("aviqtl-export".to_owned())
            .spawn(move || run_export_worker(worker_job, command_receiver, event_sender, wake))
            .map_err(|error| ExportStartError::Worker(error.to_string()))?;
        self.media_preview.reset();
        self.active = Some(ActiveExport {
            project_instance_id,
            next_frame: job.start_frame(),
            ready_for_frame: true,
            awaiting_decode: false,
            pending_audio: None,
            commands: command_sender,
            events: event_receiver,
            worker: Some(worker),
            failure_override: None,
            job,
        });
        Ok(total_frames)
    }

    pub fn next_frame_request(&mut self) -> Option<ExportFrameRequest> {
        let active = self.active.as_mut()?;
        if !active.ready_for_frame || active.awaiting_decode || active.failure_override.is_some() {
            return None;
        }
        active.awaiting_decode = true;
        Some(ExportFrameRequest {
            project_instance_id: active.project_instance_id,
            frame: active.next_frame,
        })
    }

    pub fn submit_scene(
        &mut self,
        scene: PreviewScene,
        audio_plans: Vec<AudioLayerPlan>,
        project_path: Option<&Path>,
    ) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        active.pending_audio = Some(resolve_audio_sources(audio_plans, project_path));
        self.media_preview.request_fresh(scene);
    }

    pub fn fail_current_frame(&mut self, message: String) {
        if let Some(active) = self.active.as_mut() {
            active.failure_override = Some(message);
            active.awaiting_decode = false;
            active.ready_for_frame = false;
            let _ = active.commands.send(WorkerCommand::Cancel);
        }
    }

    pub fn cancel(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.ready_for_frame = false;
            active.awaiting_decode = false;
            active.pending_audio = None;
            let _ = active.commands.send(WorkerCommand::Cancel);
        }
    }

    pub fn poll(&mut self) -> Vec<ExportEvent> {
        self.poll_decoded_frame();
        self.poll_worker_events()
    }

    fn poll_decoded_frame(&mut self) {
        let Some(batch) = self.media_preview.poll() else {
            return;
        };
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if !active.awaiting_decode {
            return;
        }
        self.preview.compose(&batch.scene, batch.generation);
        let audio_sources = active.pending_audio.take().unwrap_or_default();
        let timestamp_seconds = match &active.job {
            ExportJob::Video { fps, .. } => f64::from(active.next_frame) / fps.max(1.0),
            ExportJob::ImageSequence { .. } => 0.0,
        };
        match self.preview.read_rgba() {
            Ok(rgba) => {
                let frame = VideoFrame {
                    width: batch.scene.width.max(1),
                    height: batch.scene.height.max(1),
                    rgba,
                    timestamp_seconds,
                };
                if active
                    .commands
                    .send(WorkerCommand::Frame(ExportFrame {
                        timeline_frame: active.next_frame,
                        frame,
                        audio_sources,
                    }))
                    .is_err()
                {
                    active.failure_override = Some("Export worker stopped unexpectedly".to_owned());
                }
                active.awaiting_decode = false;
                active.ready_for_frame = false;
            }
            Err(error) => {
                active.failure_override = Some(format!("Frame grab error: {error}"));
                active.awaiting_decode = false;
                active.ready_for_frame = false;
                let _ = active.commands.send(WorkerCommand::Cancel);
            }
        }
    }

    fn poll_worker_events(&mut self) -> Vec<ExportEvent> {
        let mut public_events = Vec::new();
        let mut finished = None;
        if let Some(active) = self.active.as_mut() {
            loop {
                match active.events.try_recv() {
                    Ok(WorkerEvent::FrameProcessed { done, progress }) => {
                        if let Some(progress) = progress {
                            public_events.push(ExportEvent::Progress(progress));
                        }
                        active.next_frame = active.job.start_frame().saturating_add(done);
                        active.ready_for_frame = active.next_frame < active.job.end_frame();
                    }
                    Ok(WorkerEvent::Finished { success, message }) => {
                        finished = Some((success, message));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some((false, "Export worker stopped unexpectedly".to_owned()));
                        break;
                    }
                }
            }
        }
        let Some((mut success, mut message)) = finished else {
            return public_events;
        };
        if let Some(mut active) = self.active.take() {
            if let Some(override_message) = active.failure_override.take() {
                success = false;
                message = override_message;
            }
            if let Some(worker) = active.worker.take() {
                let _ = worker.join();
            }
        }
        self.media_preview.reset();
        public_events.push(ExportEvent::Finished(ExportResult { success, message }));
        public_events
    }
}

impl Drop for ExportManager {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            let _ = active.commands.send(WorkerCommand::Cancel);
            if let Some(worker) = active.worker.take() {
                let _ = worker.join();
            }
        }
    }
}

pub fn available_video_encoders() -> Vec<String> {
    media_video_encoders()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub fn available_audio_encoders() -> Vec<String> {
    media_audio_encoders()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub fn valid_export_path(path: &str) -> bool {
    if path.trim().is_empty() || path.contains('\0') {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        if path
            .chars()
            .any(|character| matches!(character, '<' | '>' | '"' | '|' | '?' | '*'))
        {
            return false;
        }
        if let Some(first_colon) = path.find(':') {
            let bytes = path.as_bytes();
            if first_colon != 1
                || !bytes.first().is_some_and(u8::is_ascii_alphabetic)
                || !bytes
                    .get(2)
                    .is_some_and(|value| matches!(*value, b'\\' | b'/'))
                || path[first_colon + 1..].contains(':')
            {
                return false;
            }
        }
    }
    true
}

pub fn is_software_codec(codec: &str) -> bool {
    codec.starts_with("lib") || codec == "libaom-av1"
}

pub fn nearest_audio_bitrate(value: i32) -> i32 {
    [96, 128, 192, 256, 320]
        .into_iter()
        .min_by_key(|candidate| (candidate - value).abs())
        .unwrap_or(192)
}

fn build_job(
    project: ExportProject,
    request: ExportRequest,
    settings: ExportRuntimeSettings,
) -> Result<ExportJob, ExportConfigurationError> {
    let (start_frame, end_frame) = if request.full_range {
        (0, -1)
    } else {
        (request.start_frame, request.end_frame)
    };
    match request.mode {
        ExportMode::Video => {
            let (fps_num, fps_den) = fps_rational(project.fps);
            let plan = plan_video_export(&VideoExportRequest {
                width: project.width,
                height: project.height,
                fps_num,
                fps_den,
                start_frame,
                end_frame,
                timeline_duration: project.duration,
                output_path: request.output_path.clone(),
                project_fps: project.fps,
            })?;
            Ok(ExportJob::Video {
                plan,
                config: VideoEncoderConfig {
                    width: project.width.max(1) as u32,
                    height: project.height.max(1) as u32,
                    fps_num,
                    fps_den,
                    bitrate: request.bitrate_mbps.max(1) as usize * 1_000_000,
                    crf: request.crf.map(|value| value.clamp(0, 51)),
                    gop_size: None,
                    codec_name: request.video_codec.clone(),
                    audio_codec_name: request.audio_codec,
                    audio_bitrate: request.audio_bitrate_kbps.max(1) as usize * 1_000,
                    sample_rate: project.sample_rate.max(1) as u32,
                    output_path: request.output_path,
                    preset: if is_software_codec(&request.video_codec) {
                        request.preset
                    } else {
                        String::new()
                    },
                    profile: if is_software_codec(&request.video_codec) {
                        request.profile
                    } else {
                        String::new()
                    },
                },
                fps: project.fps,
                max_plugin_block_size: settings.max_plugin_block_size.clamp(1, 8_192),
                progress_interval: settings.progress_interval.max(1),
            })
        }
        ExportMode::ImageSequence => {
            let plan = plan_image_sequence_export(&ImageSequenceExportRequest {
                start_frame,
                end_frame,
                timeline_duration: project.duration,
                configured_padding: settings.sequence_padding,
                output_directory: request.output_path.clone(),
                format: request.image_format,
            })?;
            Ok(ExportJob::ImageSequence {
                plan,
                output_directory: request.output_path,
                quality: settings.image_quality.clamp(1, 100),
                progress_interval: settings.progress_interval.max(1),
            })
        }
    }
}

fn resolve_audio_sources(
    plans: Vec<AudioLayerPlan>,
    project_path: Option<&Path>,
) -> Vec<TimelineAudioSource> {
    plans
        .into_iter()
        .filter_map(|plan| {
            let source_path = plan.source_path.as_deref()?;
            let path = resolve_project_path(project_path, source_path);
            Some(TimelineAudioSource { path, plan })
        })
        .collect()
}

fn resolve_project_path(project_path: Option<&Path>, source: &str) -> PathBuf {
    let path = PathBuf::from(source);
    if path.is_absolute() {
        path
    } else {
        project_path
            .and_then(Path::parent)
            .map_or(path.clone(), |directory| directory.join(path))
    }
}

fn run_export_worker(
    job: ExportJob,
    commands: Receiver<WorkerCommand>,
    events: Sender<WorkerEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let result = match &job {
        ExportJob::Video { config, .. } => run_video_export(&job, config, &commands, &events),
        ExportJob::ImageSequence { .. } => run_image_sequence_export(&job, &commands, &events),
    };
    let (success, message) = match result {
        Ok(()) => (true, "Export complete".to_owned()),
        Err(message) => (false, message),
    };
    let _ = events.send(WorkerEvent::Finished { success, message });
    (wake)();
}

fn run_video_export(
    job: &ExportJob,
    config: &VideoEncoderConfig,
    commands: &Receiver<WorkerCommand>,
    events: &Sender<WorkerEvent>,
) -> Result<(), String> {
    let ExportJob::Video {
        plan,
        fps,
        max_plugin_block_size,
        progress_interval,
        ..
    } = job
    else {
        return Err("internal error: video worker received a non-video job".to_owned());
    };
    let encoder = open_video_encoder(config)
        .map_err(|error| format!("Encoder error: initialization failed: {error}"))?;
    let result = encode_video_frames(
        plan,
        *fps,
        *max_plugin_block_size,
        *progress_interval,
        config,
        encoder,
        commands,
        events,
    );
    if result.is_err() {
        let _ = fs::remove_file(&config.output_path);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn encode_video_frames(
    plan: &VideoExportPlan,
    fps: f64,
    max_plugin_block_size: usize,
    progress_interval: i32,
    config: &VideoEncoderConfig,
    mut encoder: VideoEncoder,
    commands: &Receiver<WorkerCommand>,
    events: &Sender<WorkerEvent>,
) -> Result<(), String> {
    let mut mixer = TimelineAudioMixer::default();
    let timer = Instant::now();
    loop {
        match commands.recv() {
            Ok(WorkerCommand::Cancel) | Err(_) => return Err("Export cancelled".to_owned()),
            Ok(WorkerCommand::Frame(frame)) => {
                if frame.timeline_frame < plan.start_frame || frame.timeline_frame >= plan.end_frame
                {
                    return Err("Frame render error: unexpected export frame".to_owned());
                }
                let pts = i64::from(frame.timeline_frame - plan.start_frame);
                encoder.push_frame(&frame.frame, pts).map_err(|error| {
                    format!("Encoder error: failed to queue video frame: {error}")
                })?;
                let done = frame.timeline_frame - plan.start_frame + 1;
                let audio_plan = plan_export_audio_frame(
                    done - 1,
                    i32::try_from(config.sample_rate)
                        .map_err(|_| "Encoder error: audio sample rate is invalid".to_owned())?,
                    config.fps_num,
                    config.fps_den,
                )
                .ok_or_else(|| "Encoder error: audio timing failed".to_owned())?;
                if audio_plan.samples_for_frame > 0 {
                    let audio = mixer
                        .mix_frame_with_sample_count_and_plugin_block_size(
                            frame.timeline_frame,
                            fps,
                            config.sample_rate,
                            audio_plan.samples_for_frame as usize,
                            max_plugin_block_size,
                            &frame.audio_sources,
                        )
                        .map_err(|error| {
                            format!("Encoder error: audio timing failed: {error:?}")
                        })?;
                    encoder.push_audio(&audio.samples).map_err(|error| {
                        format!("Encoder error: failed to queue audio: {error}")
                    })?;
                }
                let progress = progress_for(done, plan.total_frames, progress_interval, timer);
                let _ = events.send(WorkerEvent::FrameProcessed { done, progress });
                if done == plan.total_frames {
                    encoder
                        .finish()
                        .map_err(|error| format!("Encoder error: finalization failed: {error}"))?;
                    return Ok(());
                }
            }
        }
    }
}

fn run_image_sequence_export(
    job: &ExportJob,
    commands: &Receiver<WorkerCommand>,
    events: &Sender<WorkerEvent>,
) -> Result<(), String> {
    let ExportJob::ImageSequence {
        plan,
        output_directory,
        quality,
        progress_interval,
    } = job
    else {
        return Err("internal error: image worker received a non-image job".to_owned());
    };
    let created_directory = !output_directory.exists();
    if created_directory {
        fs::create_dir_all(output_directory)
            .map_err(|_| "Output error: cannot create output directory".to_owned())?;
    }
    let output_paths = (plan.start_frame..plan.end_frame)
        .map(|frame| image_sequence_path(output_directory, frame, *plan))
        .collect::<Vec<_>>();
    if let Some(existing) = output_paths.iter().find(|path| path.exists()) {
        cleanup_image_sequence(&[], output_directory, created_directory);
        return Err(format!(
            "Output error: output file already exists: {}",
            existing.display()
        ));
    }
    let mut written = Vec::new();
    let timer = Instant::now();
    loop {
        match commands.recv() {
            Ok(WorkerCommand::Cancel) | Err(_) => {
                cleanup_image_sequence(&written, output_directory, created_directory);
                return Err("Export cancelled".to_owned());
            }
            Ok(WorkerCommand::Frame(frame)) => {
                if frame.timeline_frame < plan.start_frame || frame.timeline_frame >= plan.end_frame
                {
                    cleanup_image_sequence(&written, output_directory, created_directory);
                    return Err("Frame render error: unexpected export frame".to_owned());
                }
                let path = image_sequence_path(output_directory, frame.timeline_frame, *plan);
                let format = match plan.format {
                    ExportImageFormat::Png => StillImageFormat::Png,
                    ExportImageFormat::Jpeg => StillImageFormat::Jpeg,
                };
                if let Err(error) = save_still_image(&path, &frame.frame, format, *quality) {
                    let _ = fs::remove_file(&path);
                    cleanup_image_sequence(&written, output_directory, created_directory);
                    return Err(format!(
                        "Output error: failed to save frame {}: {error}",
                        frame.timeline_frame
                    ));
                }
                written.push(path);
                let done = frame.timeline_frame - plan.start_frame + 1;
                let progress = progress_for(done, plan.total_frames, *progress_interval, timer);
                let _ = events.send(WorkerEvent::FrameProcessed { done, progress });
                if done == plan.total_frames {
                    return Ok(());
                }
            }
        }
    }
}

fn open_video_encoder(config: &VideoEncoderConfig) -> Result<VideoEncoder, String> {
    match VideoEncoder::open(config) {
        Ok(encoder) => Ok(encoder),
        Err(primary_error) => {
            let Some(fallback) = export_codec_fallback(&config.codec_name) else {
                return Err(primary_error.to_string());
            };
            let mut fallback_config = config.clone();
            fallback_config.codec_name = fallback.to_owned();
            VideoEncoder::open(&fallback_config).map_err(|fallback_error| {
                format!(
                    "{}; fallback {} failed: {}",
                    primary_error, fallback, fallback_error
                )
            })
        }
    }
}

fn progress_for(
    done: i32,
    total_frames: i32,
    interval: i32,
    timer: Instant,
) -> Option<ExportProgressPlan> {
    let elapsed = timer.elapsed().as_millis().min(i64::MAX as u128) as i64;
    plan_export_progress(done, total_frames, interval, elapsed)
        .filter(|progress| progress.should_emit)
}

fn cleanup_image_sequence(written: &[PathBuf], directory: &Path, created_directory: bool) {
    for path in written {
        let _ = fs::remove_file(path);
    }
    if created_directory {
        let _ = fs::remove_dir(directory);
    }
}

fn image_sequence_path(directory: &Path, frame: i32, plan: ImageSequenceExportPlan) -> PathBuf {
    directory.join(format!(
        "frame_{:0width$}.{}",
        frame,
        plan.format.extension(),
        width = plan.pad_digits.max(1) as usize
    ))
}

fn fps_rational(fps: f64) -> (i32, i32) {
    if fps.fract().abs() < f64::EPSILON {
        ((fps * 1_000.0).round() as i32, 1_000)
    } else {
        ((fps * 1_001.0).round() as i32, 1_001)
    }
}

fn configuration_error_message(error: ExportConfigurationError) -> &'static str {
    match error {
        ExportConfigurationError::MissingOutputPath => "Configuration error: missing output path",
        ExportConfigurationError::InvalidOutputSize => "Configuration error: invalid output size",
        ExportConfigurationError::InvalidFps => "Configuration error: invalid FPS",
        ExportConfigurationError::InvalidRange => {
            "Configuration error: export end frame must be after start frame"
        }
        ExportConfigurationError::ProjectFpsMismatch => {
            "Configuration error: export FPS does not match project FPS"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "aviqtl-export-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn image_job(directory: PathBuf, end_frame: i32) -> ExportJob {
        ExportJob::ImageSequence {
            plan: ImageSequenceExportPlan {
                start_frame: 0,
                end_frame,
                total_frames: end_frame,
                pad_digits: 4,
                format: ExportImageFormat::Png,
            },
            output_directory: directory,
            quality: 95,
            progress_interval: 1,
        }
    }

    fn solid_frame(timeline_frame: i32) -> ExportFrame {
        ExportFrame {
            timeline_frame,
            frame: VideoFrame {
                width: 2,
                height: 2,
                rgba: [timeline_frame as u8, 40, 80, 255].repeat(4),
                timestamp_seconds: 0.0,
            },
            audio_sources: Vec::new(),
        }
    }

    fn export_request(mode: ExportMode, output_path: PathBuf) -> ExportRequest {
        ExportRequest {
            mode,
            output_path,
            image_format: ExportImageFormat::Png,
            video_codec: "libx264".to_owned(),
            crf: Some(20),
            bitrate_mbps: 15,
            preset: "medium".to_owned(),
            profile: "high".to_owned(),
            audio_codec: "aac".to_owned(),
            audio_bitrate_kbps: 192,
            full_range: true,
            start_frame: 0,
            end_frame: 300,
        }
    }

    #[test]
    fn qt_fps_rational_is_preserved() {
        assert_eq!(fps_rational(60.0), (60_000, 1_000));
        assert_eq!(fps_rational(60_000.0 / 1_001.0), (60_000, 1_001));
    }

    #[test]
    fn sequence_names_use_qt_padding_and_extensions() {
        let plan = ImageSequenceExportPlan {
            start_frame: 0,
            end_frame: 10,
            total_frames: 10,
            pad_digits: 6,
            format: ExportImageFormat::Png,
        };
        assert_eq!(
            image_sequence_path(Path::new("frames"), 12, plan),
            PathBuf::from("frames/frame_000012.png")
        );
        assert!(!valid_export_path(""));
        assert!(valid_export_path("out/video.mp4"));
    }

    #[test]
    fn video_request_preserves_qt_units_ranges_and_hardware_options() {
        let mut request =
            export_request(ExportMode::Video, PathBuf::from("out/hardware-export.mp4"));
        request.video_codec = "h264_videotoolbox".to_owned();
        request.crf = None;
        request.bitrate_mbps = 23;
        request.audio_bitrate_kbps = 256;
        request.full_range = false;
        request.start_frame = 12;
        request.end_frame = 72;
        let project = ExportProject {
            width: 1_280,
            height: 720,
            fps: 60_000.0 / 1_001.0,
            duration: 300,
            sample_rate: 48_000,
        };

        let job = build_job(project, request, ExportRuntimeSettings::default())
            .expect("video request is valid");
        let ExportJob::Video { plan, config, .. } = job else {
            panic!("video request builds a video job");
        };
        assert_eq!(
            (plan.start_frame, plan.end_frame, plan.total_frames),
            (12, 72, 60)
        );
        assert_eq!((config.fps_num, config.fps_den), (60_000, 1_001));
        assert_eq!(config.bitrate, 23_000_000);
        assert_eq!(config.audio_bitrate, 256_000);
        assert_eq!(config.crf, None);
        assert!(config.preset.is_empty());
        assert!(config.profile.is_empty());
    }

    #[test]
    fn worker_reports_progress_then_completion_and_wakes_the_ui() {
        let directory = temporary_directory("state-flow");
        let job = image_job(directory.clone(), 1);
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        command_sender
            .send(WorkerCommand::Frame(solid_frame(0)))
            .expect("frame queues");
        let woke = Arc::new(AtomicBool::new(false));
        let wake_state = woke.clone();

        run_export_worker(
            job,
            command_receiver,
            event_sender,
            Arc::new(move || wake_state.store(true, Ordering::SeqCst)),
        );

        assert!(matches!(
            event_receiver.try_recv(),
            Ok(WorkerEvent::FrameProcessed { done: 1, .. })
        ));
        assert!(matches!(
            event_receiver.try_recv(),
            Ok(WorkerEvent::Finished { success: true, .. })
        ));
        assert!(woke.load(Ordering::SeqCst));
        fs::remove_dir_all(directory).expect("generated sequence removes");
    }

    #[test]
    fn image_worker_writes_the_full_sequence_and_reports_each_frame() {
        let directory = temporary_directory("complete");
        let job = image_job(directory.clone(), 2);
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        command_sender
            .send(WorkerCommand::Frame(solid_frame(0)))
            .expect("first frame queues");
        command_sender
            .send(WorkerCommand::Frame(solid_frame(1)))
            .expect("second frame queues");
        run_image_sequence_export(&job, &command_receiver, &event_sender)
            .expect("sequence export completes");
        assert!(directory.join("frame_0000.png").exists());
        assert!(directory.join("frame_0001.png").exists());
        assert!(matches!(
            event_receiver.try_recv(),
            Ok(WorkerEvent::FrameProcessed { done: 1, .. })
        ));
        assert!(matches!(
            event_receiver.try_recv(),
            Ok(WorkerEvent::FrameProcessed { done: 2, .. })
        ));
        fs::remove_dir_all(directory).expect("generated sequence removes");
    }

    #[test]
    fn image_worker_removes_partial_output_after_confirmed_cancellation() {
        let directory = temporary_directory("cancelled");
        let job = image_job(directory.clone(), 3);
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, _event_receiver) = mpsc::channel();
        command_sender
            .send(WorkerCommand::Frame(solid_frame(0)))
            .expect("first frame queues");
        command_sender
            .send(WorkerCommand::Cancel)
            .expect("cancellation queues");
        assert_eq!(
            run_image_sequence_export(&job, &command_receiver, &event_sender),
            Err("Export cancelled".to_owned())
        );
        assert!(!directory.exists());
    }

    #[test]
    fn video_worker_removes_partial_output_after_confirmed_cancellation() {
        if !media_video_encoders().contains(&"libx264") || !media_audio_encoders().contains(&"aac")
        {
            return;
        }
        let path = temporary_directory("cancelled-video").with_extension("mp4");
        let config = VideoEncoderConfig {
            width: 64,
            height: 64,
            fps_num: 60_000,
            fps_den: 1_000,
            bitrate: 500_000,
            crf: Some(28),
            gop_size: None,
            codec_name: "libx264".to_owned(),
            audio_codec_name: "aac".to_owned(),
            audio_bitrate: 96_000,
            sample_rate: 48_000,
            output_path: path.clone(),
            preset: "ultrafast".to_owned(),
            profile: String::new(),
        };
        let job = ExportJob::Video {
            plan: VideoExportPlan {
                start_frame: 0,
                end_frame: 2,
                total_frames: 2,
            },
            config: config.clone(),
            fps: 60.0,
            max_plugin_block_size: 1_024,
            progress_interval: 1,
        };
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, _event_receiver) = mpsc::channel();
        command_sender
            .send(WorkerCommand::Frame(ExportFrame {
                timeline_frame: 0,
                frame: VideoFrame {
                    width: 64,
                    height: 64,
                    rgba: [40, 80, 120, 255].repeat(64 * 64),
                    timestamp_seconds: 0.0,
                },
                audio_sources: Vec::new(),
            }))
            .expect("first frame queues");
        command_sender
            .send(WorkerCommand::Cancel)
            .expect("cancellation queues");

        assert_eq!(
            run_video_export(&job, &config, &command_receiver, &event_sender),
            Err("Export cancelled".to_owned())
        );
        assert!(!path.exists());
    }
}
