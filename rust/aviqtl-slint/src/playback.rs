//! Preview, audio output, and background waveform coordination.

use crate::{MainWindow, TimelineWindow};
use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::settings::SettingsStore;
use aviqtl_app::{ApplicationModel, ProjectDocument};
use aviqtl_audio::{AudioLayerPlan, AudioOutput, TimelineAudioMixer, TimelineAudioSource};
use aviqtl_preview::{MediaPreview, PreviewPlanner, PreviewSurface};
use slint::{Model, ModelRc, VecModel};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, mpsc};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PreviewSourceKey {
    pub(super) project_instance_id: u64,
    pub(super) document_revision: u64,
    pub(super) project_path: Option<PathBuf>,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PreviewFrameKey {
    pub(super) source: PreviewSourceKey,
    pub(super) scene_id: i32,
    pub(super) frame: i32,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PreviewSceneKey {
    pub(super) source: PreviewSourceKey,
    pub(super) scene_id: i32,
}

pub(super) struct PreviewRuntime {
    pub(super) surface: PreviewSurface,
    pub(super) decoder: MediaPreview,
    pub(super) planner: Option<PreviewPlanner>,
    pub(super) source_key: Option<PreviewSourceKey>,
    pub(super) requested_frame: Option<PreviewFrameKey>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct AudioMeterSnapshot {
    pub(super) master_peak_left: f32,
    pub(super) master_peak_right: f32,
    pub(super) master_rms_left: f32,
    pub(super) master_rms_right: f32,
    pub(super) selected_peak_left: f32,
    pub(super) selected_peak_right: f32,
    pub(super) selected_rms_left: f32,
    pub(super) selected_rms_right: f32,
}

pub(super) struct AudioPlaybackRuntime {
    pub(super) output: Option<AudioOutput>,
    pub(super) last_output_attempt: Option<Instant>,
    pub(super) output_error: String,
    pub(super) planner: Option<PreviewPlanner>,
    pub(super) mixer: TimelineAudioMixer,
    pub(super) source_key: Option<PreviewSceneKey>,
    pub(super) timeline_rate: Option<f64>,
    pub(super) next_frame: Option<i32>,
    pub(super) was_playing: bool,
    pub(super) master_levels: [f32; 4],
    pub(super) track_levels: HashMap<i32, [f32; 4]>,
}

pub(super) struct TimelineWaveformResult {
    pub(super) source_key: PreviewSceneKey,
    pub(super) peaks: HashMap<i32, Vec<f32>>,
}

pub(super) struct TimelineWaveformRuntime {
    pub(super) source_key: Option<PreviewSceneKey>,
    pub(super) pending_key: Option<PreviewSceneKey>,
    pub(super) completed_key: Option<PreviewSceneKey>,
    pub(super) peaks: HashMap<i32, Vec<f32>>,
    pub(super) receiver: Option<Receiver<TimelineWaveformResult>>,
    pub(super) worker: Option<JoinHandle<()>>,
    pub(super) worker_stop: Option<Arc<AtomicBool>>,
}

impl PreviewRuntime {
    pub(super) fn new(surface: PreviewSurface) -> Self {
        Self {
            surface,
            decoder: MediaPreview::new(Arc::new(|| {})),
            planner: None,
            source_key: None,
            requested_frame: None,
        }
    }

    pub(super) fn set_render_settings(&mut self, render_scale: f32, msaa_samples: u32) {
        let changed = self.surface.set_render_scale(render_scale)
            | self.surface.set_msaa_samples(msaa_samples);
        if changed {
            self.decoder.reset();
            self.requested_frame = None;
        }
    }

    pub(super) fn invalidate_native_catalog(&mut self) {
        self.decoder.reset();
        self.planner = None;
        self.source_key = None;
        self.requested_frame = None;
    }

    pub(super) fn update(
        &mut self,
        model: &ApplicationModel,
        main: &MainWindow,
        effect_catalog: &EffectCatalog,
    ) -> bool {
        let Some(project_instance_id) = model.current_project_instance_id() else {
            if self.source_key.is_some() || self.requested_frame.is_some() || self.planner.is_some()
            {
                self.decoder.reset();
                self.planner = None;
                self.source_key = None;
                self.requested_frame = None;
            }
            return false;
        };
        let Some(workspace) = model.current_workspace() else {
            return false;
        };
        let source_key = PreviewSourceKey {
            project_instance_id,
            document_revision: workspace.document_revision(),
            project_path: workspace.project().path.clone(),
        };
        if self.source_key.as_ref() != Some(&source_key) {
            self.decoder.reset();
            if let Some(planner) = self.planner.as_mut() {
                planner.rebuild(workspace.document(), workspace.project().path.as_deref());
                planner.set_native_definitions(effect_catalog.native_definitions());
            } else {
                let mut planner =
                    PreviewPlanner::new(workspace.document(), workspace.project().path.as_deref());
                planner.set_native_definitions(effect_catalog.native_definitions());
                self.planner = Some(planner);
            }
            self.source_key = Some(source_key.clone());
            self.requested_frame = None;
        }
        let frame_key = PreviewFrameKey {
            source: source_key,
            scene_id: workspace.selected_scene(),
            frame: workspace.playhead(),
        };
        if self.requested_frame.as_ref() != Some(&frame_key) {
            if let Some(planned) = self.planner.as_mut().and_then(|planner| {
                planner.build(
                    workspace.document(),
                    workspace.selected_scene(),
                    workspace.playhead(),
                )
            }) {
                for warning in planned.warnings {
                    eprintln!("Preview planning warning: {warning}");
                }
                self.decoder.request(planned.scene);
            }
            self.requested_frame = Some(frame_key);
        }
        let Some(batch) = self.decoder.poll() else {
            return false;
        };
        for error in &batch.errors {
            eprintln!("Preview decode warning: {error}");
        }
        let target_replaced = self.surface.compose(&batch.scene, batch.generation);
        if target_replaced {
            match slint::Image::try_from(self.surface.texture().clone()) {
                Ok(image) => main.set_preview_image(image),
                Err(error) => eprintln!("Preview texture import failed: {error}"),
            }
        }
        true
    }
}

impl AudioPlaybackRuntime {
    pub(super) fn new() -> Self {
        Self {
            output: None,
            last_output_attempt: None,
            output_error: String::new(),
            planner: None,
            mixer: TimelineAudioMixer::default(),
            source_key: None,
            timeline_rate: None,
            next_frame: None,
            was_playing: false,
            master_levels: [0.0; 4],
            track_levels: HashMap::new(),
        }
    }

    pub(super) fn output_error(&self) -> &str {
        &self.output_error
    }

    pub(super) fn update(
        &mut self,
        model: &ApplicationModel,
        settings: &SettingsStore,
    ) -> AudioMeterSnapshot {
        let Some(project_instance_id) = model.current_project_instance_id() else {
            self.stop();
            return AudioMeterSnapshot::default();
        };
        let Some(workspace) = model.current_workspace() else {
            self.stop();
            return AudioMeterSnapshot::default();
        };
        if !workspace.is_playing() {
            self.stop();
            return AudioMeterSnapshot::default();
        }
        if !self.ensure_output() {
            return AudioMeterSnapshot::default();
        }

        let source_key = PreviewSceneKey {
            source: PreviewSourceKey {
                project_instance_id,
                document_revision: workspace.document_revision(),
                project_path: workspace.project().path.clone(),
            },
            scene_id: workspace.selected_scene(),
        };
        if self.source_key.as_ref() != Some(&source_key) {
            if let Some(planner) = self.planner.as_mut() {
                planner.rebuild(workspace.document(), workspace.project().path.as_deref());
            } else {
                self.planner = Some(PreviewPlanner::new(
                    workspace.document(),
                    workspace.project().path.as_deref(),
                ));
            }
            self.source_key = Some(source_key);
            self.reset_queue();
        }

        let sample_rate = self
            .output
            .as_ref()
            .map_or(48_000, AudioOutput::sample_rate);
        let fps = scene_fps(workspace.document(), workspace.selected_scene());
        let timeline_rate = workspace.playback_speed().clamp(0.1, 4.0);
        let target_frames = (sample_rate as usize / 8).max(1);
        let permitted_lead =
            audio_queue_lead_frames(fps, timeline_rate, target_frames, sample_rate);
        let current_frame = workspace.playhead();
        if self.timeline_rate != Some(timeline_rate) {
            self.reset_queue();
            self.next_frame = Some(current_frame);
            self.timeline_rate = Some(timeline_rate);
        }
        if self.next_frame.is_none_or(|next_frame| {
            next_frame < current_frame || next_frame > current_frame.saturating_add(permitted_lead)
        }) {
            self.reset_queue();
            self.next_frame = Some(current_frame);
        }
        let timeline_end = workspace.timeline_duration();
        let max_plugin_block_size = settings
            .i32_value("audioPluginMaxBlockSize", 1024)
            .clamp(1, 8_192) as usize;

        while self
            .output
            .as_ref()
            .is_some_and(|output| output.queued_frames() < target_frames)
        {
            let frame = self.next_frame.unwrap_or(current_frame);
            if frame >= timeline_end {
                break;
            }
            let Some(output_frames) =
                samples_for_timeline_frame(frame, fps, sample_rate, timeline_rate)
            else {
                break;
            };
            let Some(planned) = self.planner.as_mut().and_then(|planner| {
                planner.build(workspace.document(), workspace.selected_scene(), frame)
            }) else {
                break;
            };
            for warning in planned.warnings {
                eprintln!("Audio preview planning warning: {warning}");
            }
            let sources =
                resolve_timeline_audio_sources(planned.audio, workspace.project().path.as_deref());
            match self.mixer.mix_frame_with_timeline_rate(
                frame,
                fps,
                sample_rate,
                output_frames,
                max_plugin_block_size,
                timeline_rate,
                &sources,
            ) {
                Ok(block) => {
                    for error in block.errors {
                        eprintln!("Audio preview warning: {error}");
                    }
                    self.master_levels = stereo_levels(&block.samples);
                    self.track_levels = block
                        .meters
                        .into_iter()
                        .map(|meter| {
                            (
                                meter.clip_id,
                                [
                                    meter.peak_left,
                                    meter.peak_right,
                                    meter.rms_left,
                                    meter.rms_right,
                                ],
                            )
                        })
                        .collect();
                    if let Some(output) = self.output.as_mut() {
                        output.enqueue_stereo(&block.samples);
                    }
                }
                Err(error) => eprintln!("Audio preview timing warning: {error:?}"),
            }
            self.next_frame = Some(frame.saturating_add(1));
        }
        self.was_playing = true;

        let selected = workspace
            .selected_clip_document()
            .and_then(|clip| self.track_levels.get(&clip.id).copied())
            .unwrap_or([0.0; 4]);
        AudioMeterSnapshot {
            master_peak_left: self.master_levels[0],
            master_peak_right: self.master_levels[1],
            master_rms_left: self.master_levels[2],
            master_rms_right: self.master_levels[3],
            selected_peak_left: selected[0],
            selected_peak_right: selected[1],
            selected_rms_left: selected[2],
            selected_rms_right: selected[3],
        }
    }

    pub(super) fn ensure_output(&mut self) -> bool {
        if self.output.is_some() {
            return true;
        }
        let now = Instant::now();
        if self
            .last_output_attempt
            .is_some_and(|attempt| now.duration_since(attempt) < Duration::from_secs(5))
        {
            return false;
        }
        self.last_output_attempt = Some(now);
        match AudioOutput::open_default() {
            Ok(output) => {
                self.output = Some(output);
                self.output_error.clear();
                true
            }
            Err(error) => {
                if self.output_error != error {
                    eprintln!("Audio preview unavailable: {error}");
                }
                self.output_error = error;
                false
            }
        }
    }

    pub(super) fn reset_queue(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.clear();
        }
        self.mixer = TimelineAudioMixer::default();
        self.next_frame = None;
        self.master_levels = [0.0; 4];
        self.track_levels.clear();
    }

    pub(super) fn stop(&mut self) {
        if self.was_playing || self.next_frame.is_some() {
            self.reset_queue();
        }
        self.was_playing = false;
    }
}

impl TimelineWaveformRuntime {
    pub(super) fn new() -> Self {
        Self {
            source_key: None,
            pending_key: None,
            completed_key: None,
            peaks: HashMap::new(),
            receiver: None,
            worker: None,
            worker_stop: None,
        }
    }

    pub(super) fn update(&mut self, timeline: &TimelineWindow, model: &ApplicationModel) {
        self.poll();
        let Some(project_instance_id) = model.current_project_instance_id() else {
            self.clear(timeline);
            return;
        };
        let Some(workspace) = model.current_workspace() else {
            self.clear(timeline);
            return;
        };
        let source_key = PreviewSceneKey {
            source: PreviewSourceKey {
                project_instance_id,
                document_revision: workspace.document_revision(),
                project_path: workspace.project().path.clone(),
            },
            scene_id: workspace.selected_scene(),
        };
        if self.source_key.as_ref() != Some(&source_key) {
            self.cancel_pending();
            self.source_key = Some(source_key.clone());
            self.completed_key = None;
            self.peaks.clear();
            clear_timeline_waveforms(timeline);
        }
        if self.pending_key.is_none() && self.completed_key.as_ref() != Some(&source_key) {
            self.start(
                source_key,
                workspace.document().clone(),
                workspace.project().path.clone(),
            );
        }
        apply_timeline_waveforms(timeline, &self.peaks);
    }

    pub(super) fn start(
        &mut self,
        source_key: PreviewSceneKey,
        document: ProjectDocument,
        project_path: Option<PathBuf>,
    ) {
        let (sender, receiver) = mpsc::channel();
        let worker_key = source_key.clone();
        let worker_stop = Arc::new(AtomicBool::new(false));
        let worker_stop_flag = Arc::clone(&worker_stop);
        match thread::Builder::new()
            .name("aviqtl-waveform-builder".to_owned())
            .spawn(move || {
                let Some(peaks) = build_timeline_waveforms(
                    &document,
                    worker_key.scene_id,
                    project_path.as_deref(),
                    &worker_stop_flag,
                ) else {
                    return;
                };
                let _ = sender.send(TimelineWaveformResult {
                    source_key: worker_key,
                    peaks,
                });
            }) {
            Ok(worker) => {
                self.pending_key = Some(source_key);
                self.receiver = Some(receiver);
                self.worker = Some(worker);
                self.worker_stop = Some(worker_stop);
            }
            Err(error) => {
                eprintln!("Failed to start waveform builder: {error}");
                self.completed_key = Some(source_key);
            }
        }
    }

    pub(super) fn poll(&mut self) {
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        if !worker.is_finished() {
            return;
        }
        let worker = self.worker.take().expect("waveform worker remains present");
        let _ = worker.join();
        self.worker_stop = None;
        let result = self
            .receiver
            .take()
            .and_then(|receiver| receiver.try_recv().ok());
        self.pending_key = None;
        let Some(result) = result else {
            return;
        };
        if self.source_key.as_ref() == Some(&result.source_key) {
            self.completed_key = Some(result.source_key);
            self.peaks = result.peaks;
        }
    }

    pub(super) fn cancel_pending(&mut self) {
        if let Some(stop) = self.worker_stop.as_ref() {
            stop.store(true, Ordering::Release);
        }
    }

    pub(super) fn clear(&mut self, timeline: &TimelineWindow) {
        self.cancel_pending();
        if self.source_key.take().is_some() || !self.peaks.is_empty() {
            self.completed_key = None;
            self.peaks.clear();
            clear_timeline_waveforms(timeline);
        }
    }
}

pub(super) fn scene_fps(document: &ProjectDocument, scene_id: i32) -> f64 {
    document
        .scenes
        .iter()
        .find(|scene| scene.id == scene_id)
        .map_or(document.settings.fps, |scene| scene.fps)
        .max(1.0)
}

pub(super) fn audio_queue_lead_frames(
    fps: f64,
    timeline_rate: f64,
    target_frames: usize,
    sample_rate: u32,
) -> i32 {
    if !fps.is_finite()
        || fps <= 0.0
        || !timeline_rate.is_finite()
        || timeline_rate <= 0.0
        || sample_rate == 0
    {
        return 1;
    }
    let timeline_frames =
        (target_frames as f64 * fps * timeline_rate / f64::from(sample_rate)).ceil();
    if timeline_frames.is_finite() {
        timeline_frames.clamp(1.0, f64::from(i32::MAX)) as i32
    } else {
        i32::MAX
    }
}

pub(super) fn samples_for_timeline_frame(
    frame: i32,
    fps: f64,
    sample_rate: u32,
    timeline_rate: f64,
) -> Option<usize> {
    if frame < 0
        || !fps.is_finite()
        || fps <= 0.0
        || sample_rate == 0
        || !timeline_rate.is_finite()
        || timeline_rate <= 0.0
    {
        return None;
    }
    let output_fps = fps * timeline_rate;
    let start = (f64::from(frame) * f64::from(sample_rate) / output_fps).floor();
    let end = (f64::from(frame.saturating_add(1)) * f64::from(sample_rate) / output_fps).floor();
    usize::try_from((end - start).max(1.0) as u64).ok()
}

fn resolve_timeline_audio_sources(
    plans: Vec<AudioLayerPlan>,
    project_path: Option<&Path>,
) -> Vec<TimelineAudioSource> {
    plans
        .into_iter()
        .filter_map(|plan| {
            let source = plan.source_path.as_deref()?;
            let source = PathBuf::from(source);
            let path = if source.is_absolute() {
                source
            } else {
                project_path
                    .and_then(Path::parent)
                    .map_or(source.clone(), |directory| directory.join(source))
            };
            Some(TimelineAudioSource { path, plan })
        })
        .collect()
}

pub(super) fn stereo_levels(samples: &[f32]) -> [f32; 4] {
    let mut peak_left = 0.0_f32;
    let mut peak_right = 0.0_f32;
    let mut square_left = 0.0_f64;
    let mut square_right = 0.0_f64;
    let mut frames = 0_usize;
    for frame in samples.chunks_exact(2) {
        peak_left = peak_left.max(frame[0].abs());
        peak_right = peak_right.max(frame[1].abs());
        square_left += f64::from(frame[0]) * f64::from(frame[0]);
        square_right += f64::from(frame[1]) * f64::from(frame[1]);
        frames += 1;
    }
    if frames == 0 {
        return [0.0; 4];
    }
    [
        peak_left,
        peak_right,
        (square_left / frames as f64).sqrt() as f32,
        (square_right / frames as f64).sqrt() as f32,
    ]
}

fn build_timeline_waveforms(
    document: &ProjectDocument,
    scene_id: i32,
    project_path: Option<&Path>,
    stop: &AtomicBool,
) -> Option<HashMap<i32, Vec<f32>>> {
    const POINTS: usize = 96;
    if stop.load(Ordering::Acquire) {
        return None;
    }
    let fps = scene_fps(document, scene_id);
    let sample_rate = document.settings.sample_rate.max(1) as u32;
    let mut planner = PreviewPlanner::new(document, project_path);
    let mut mixer = TimelineAudioMixer::default();
    let mut waveforms = HashMap::new();
    for clip in document
        .clips
        .iter()
        .filter(|clip| clip.scene_id == scene_id && clip.clip_type == "audio")
    {
        if stop.load(Ordering::Acquire) {
            return None;
        }
        let mut peaks = Vec::with_capacity(POINTS);
        for point in 0..POINTS {
            if stop.load(Ordering::Acquire) {
                return None;
            }
            let relative_frame = ((point as i64 * i64::from(clip.duration.max(1))) / POINTS as i64)
                .clamp(0, i64::from(clip.duration.saturating_sub(1).max(0)))
                as i32;
            let frame = clip.start.saturating_add(relative_frame);
            let peak = if let Some(mut plan) =
                planner
                    .build(document, scene_id, frame)
                    .and_then(|planned| {
                        planned
                            .audio
                            .into_iter()
                            .find(|plan| plan.clip_id == clip.id)
                    }) {
                plan.plugins.clear();
                let sources = resolve_timeline_audio_sources(vec![plan], project_path);
                let sample_frames =
                    (f64::from(sample_rate) / fps).ceil().clamp(1.0, 8_192.0) as usize;
                mixer
                    .mix_frame_with_sample_count_and_plugin_block_size(
                        frame,
                        fps,
                        sample_rate,
                        sample_frames,
                        8_192,
                        &sources,
                    )
                    .ok()
                    .map(|block| {
                        block
                            .samples
                            .iter()
                            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
                            .clamp(0.0, 1.0)
                    })
                    .unwrap_or_default()
            } else {
                0.0
            };
            peaks.push(peak);
        }
        waveforms.insert(clip.id, peaks);
    }
    Some(waveforms)
}

fn clear_timeline_waveforms(timeline: &TimelineWindow) {
    let clips = timeline.get_clips();
    for index in 0..clips.row_count() {
        let Some(mut clip) = clips.row_data(index) else {
            continue;
        };
        if clip.waveform.row_count() > 0 {
            clip.waveform = ModelRc::new(VecModel::<f32>::default());
            clips.set_row_data(index, clip);
        }
    }
}

fn apply_timeline_waveforms(timeline: &TimelineWindow, peaks: &HashMap<i32, Vec<f32>>) {
    let clips = timeline.get_clips();
    for index in 0..clips.row_count() {
        let Some(mut clip) = clips.row_data(index) else {
            continue;
        };
        let Some(waveform) = peaks.get(&clip.id) else {
            continue;
        };
        if clip.waveform.row_count() != waveform.len() {
            clip.waveform = ModelRc::new(VecModel::from(waveform.clone()));
            clips.set_row_data(index, clip);
        }
    }
}
