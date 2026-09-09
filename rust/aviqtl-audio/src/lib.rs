//! Timeline-synchronous audio decoding and mixing independent of an output device.

mod plugin_host;

use aviqtl_media::AudioDecoder;
use aviqtl_rust_core::api::{
    AudioLayerPlan, StereoMixParameters, StereoMixTrack, StereoTrackMeter, mix_stereo_tracks,
    resample_stereo,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use plugin_host::{AudioPluginChain, AudioPluginProcessContext};
pub use plugin_host::{
    AudioPluginDescription, AudioPluginParameterInfo, ModernAudioPluginFormat,
    ModernAudioPluginInfo, inspect_audio_plugin, scan_modern_audio_plugins,
};

/// A resolved media path paired with its evaluated Rust timeline plan.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineAudioSource {
    pub path: PathBuf,
    pub plan: AudioLayerPlan,
}

/// One frame-sized block ready to enqueue for an audio output device.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineAudioBlock {
    pub timeline_frame: i32,
    pub start_sample: i64,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    pub meters: Vec<StereoTrackMeter>,
    pub errors: Vec<String>,
}

/// Errors that prevent the whole timeline frame from being planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineAudioError {
    InvalidTiming,
}

struct DecodedTrack {
    plan: AudioLayerPlan,
    samples: Vec<f32>,
}

/// Reuses per-source FFmpeg decoders while producing independently owned mix blocks.
#[derive(Default)]
pub struct TimelineAudioMixer {
    decoders: HashMap<PathBuf, AudioDecoder>,
    plugin_chains: HashMap<i32, AudioPluginChain>,
}

impl TimelineAudioMixer {
    pub fn mix_frame_with_sample_count_and_plugin_block_size(
        &mut self,
        timeline_frame: i32,
        fps: f64,
        sample_rate: u32,
        output_frames: usize,
        max_plugin_block_size: usize,
        sources: &[TimelineAudioSource],
    ) -> Result<TimelineAudioBlock, TimelineAudioError> {
        let Some(start_sample) = cumulative_samples(timeline_frame, fps, sample_rate) else {
            return Err(TimelineAudioError::InvalidTiming);
        };
        self.mix_planned_frame(
            timeline_frame,
            fps,
            sample_rate,
            start_sample,
            output_frames,
            max_plugin_block_size,
            sources,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn mix_planned_frame(
        &mut self,
        timeline_frame: i32,
        fps: f64,
        sample_rate: u32,
        start_sample: i64,
        output_frames: usize,
        max_plugin_block_size: usize,
        sources: &[TimelineAudioSource],
    ) -> Result<TimelineAudioBlock, TimelineAudioError> {
        let mut decoded_tracks = Vec::with_capacity(sources.len());
        let mut errors = Vec::new();
        for source in sources {
            let source_rate = if source.plan.direct_mode {
                1.0
            } else {
                f64::from(source.plan.playback_speed)
            };
            let resample = (source_rate - 1.0).abs() > 0.01;
            let source_frames = if resample {
                ((output_frames as f64 * source_rate).ceil() as usize)
                    .saturating_add(2)
                    .max(1)
            } else {
                output_frames
            };
            let source_time = source_time_seconds(&source.plan, timeline_frame, fps);
            let decoded = self
                .decoder(&source.path, sample_rate)
                .and_then(|decoder| decoder.decode_range(source_time, source_frames));
            let mut samples = match decoded {
                Ok(decoded) if !resample => {
                    decoded.samples[..output_frames.saturating_mul(2)].to_vec()
                }
                Ok(decoded) => resample_stereo(&decoded.samples, output_frames, source_rate)
                    .unwrap_or_else(|error| {
                        errors.push(format!(
                            "clip #{} ({}): audio resample failed: {error:?}",
                            source.plan.clip_id,
                            source.path.display()
                        ));
                        vec![0.0; output_frames.saturating_mul(2)]
                    }),
                Err(error) => {
                    errors.push(format!(
                        "clip #{} ({}): {error}",
                        source.plan.clip_id,
                        source.path.display()
                    ));
                    vec![0.0; output_frames.saturating_mul(2)]
                }
            };
            if !source.plan.plugins.is_empty() {
                self.plugin_chains
                    .entry(source.plan.clip_id)
                    .or_default()
                    .process(
                        &source.plan.plugins,
                        AudioPluginProcessContext {
                            timeline_frame,
                            start_sample,
                            sample_rate,
                            max_block_size: max_plugin_block_size,
                        },
                        &mut samples,
                        &mut errors,
                    );
            } else {
                self.plugin_chains.remove(&source.plan.clip_id);
            }
            decoded_tracks.push(DecodedTrack {
                plan: source.plan.clone(),
                samples,
            });
        }
        self.plugin_chains.retain(|clip_id, _| {
            sources
                .iter()
                .any(|source| source.plan.clip_id == *clip_id && !source.plan.plugins.is_empty())
        });

        let tracks = decoded_tracks
            .iter()
            .map(|track| StereoMixTrack {
                clip_id: track.plan.clip_id,
                samples: &track.samples,
                parameters: mix_parameters(&track.plan, timeline_frame, fps),
                mute: track.plan.mute,
                solo: track.plan.solo,
            })
            .collect::<Vec<_>>();
        let mixed = mix_stereo_tracks(&tracks, output_frames)
            .expect("timeline mixer constructs only interleaved stereo buffers");
        Ok(TimelineAudioBlock {
            timeline_frame,
            start_sample,
            sample_rate,
            samples: mixed.samples,
            meters: mixed.meters,
            errors,
        })
    }

    fn decoder(
        &mut self,
        path: &Path,
        sample_rate: u32,
    ) -> Result<&mut AudioDecoder, aviqtl_media::MediaError> {
        let replace = self
            .decoders
            .get(path)
            .is_some_and(|decoder| decoder.sample_rate() != sample_rate);
        if replace {
            self.decoders.remove(path);
        }
        if !self.decoders.contains_key(path) {
            self.decoders
                .insert(path.to_path_buf(), AudioDecoder::open(path, sample_rate)?);
        }
        Ok(self
            .decoders
            .get_mut(path)
            .expect("audio decoder was inserted above"))
    }
}

#[cfg(test)]
fn frame_sample_range(timeline_frame: i32, fps: f64, sample_rate: u32) -> Option<(i64, usize)> {
    // Test-only helper: production mixes through
    // mix_frame_with_sample_count_and_plugin_block_size with an explicit
    // frame count, so the implicit range lookup survives only for tests.
    if timeline_frame < 0 || !fps.is_finite() || fps <= 0.0 || sample_rate == 0 {
        return None;
    }
    let start = cumulative_samples(timeline_frame, fps, sample_rate)?;
    let end = cumulative_samples(timeline_frame.checked_add(1)?, fps, sample_rate)?;
    let length = usize::try_from(end.checked_sub(start)?).ok()?;
    Some((start, length))
}

fn cumulative_samples(frame: i32, fps: f64, sample_rate: u32) -> Option<i64> {
    let samples = f64::from(frame) * f64::from(sample_rate) / fps;
    if !samples.is_finite() || samples < 0.0 || samples > i64::MAX as f64 {
        return None;
    }
    Some(samples.floor() as i64)
}

fn source_time_seconds(plan: &AudioLayerPlan, timeline_frame: i32, fps: f64) -> f64 {
    if plan.direct_mode {
        return f64::from(plan.direct_time);
    }
    let relative_frames = timeline_frame.saturating_sub(plan.start_frame).max(0);
    f64::from(plan.source_start_time)
        + f64::from(relative_frames) / fps * f64::from(plan.playback_speed)
}

fn mix_parameters(plan: &AudioLayerPlan, timeline_frame: i32, fps: f64) -> StereoMixParameters {
    StereoMixParameters {
        relative_time: f64::from(timeline_frame.saturating_sub(plan.start_frame).max(0)) / fps,
        duration: f64::from(plan.duration_frames.max(0)) / fps,
        fade_in_seconds: plan.fade_in_seconds,
        fade_out_seconds: plan.fade_out_seconds,
        volume: plan.volume,
        master_volume: plan.master_volume,
        pan: plan.pan,
        limiter: plan.limiter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::DEFAULT_PLUGIN_BLOCK_SIZE;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn plan(clip_id: i32) -> AudioLayerPlan {
        AudioLayerPlan {
            clip_id,
            source_path: Some("missing.wav".to_owned()),
            start_frame: 0,
            duration_frames: 120,
            source_start_time: 1.0,
            playback_speed: 2.0,
            direct_time: 3.0,
            volume: 1.0,
            master_volume: 1.0,
            pan: 0.0,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
            mute: false,
            solo: false,
            limiter: true,
            direct_mode: false,
            plugins: Vec::new(),
        }
    }

    #[test]
    fn cumulative_frame_ranges_do_not_drift_at_fractional_fps() {
        let fps = 30_000.0 / 1_001.0;
        let (start, count) = frame_sample_range(29_999, fps, 48_000).expect("timing is valid");
        assert_eq!(start + count as i64, 48_048_000);
    }

    #[test]
    fn export_sample_count_can_start_at_an_arbitrary_timeline_frame() {
        let fps = 60_000.0 / 1_001.0;
        let mut mixer = TimelineAudioMixer::default();
        let (_, preview_frames) =
            frame_sample_range(1, fps, 48_000).expect("timing is valid");
        let preview = mixer
            .mix_frame_with_sample_count_and_plugin_block_size(
                1,
                fps,
                48_000,
                preview_frames,
                DEFAULT_PLUGIN_BLOCK_SIZE,
                &[],
            )
            .expect("timeline audio frame mixes");
        let export = mixer
            .mix_frame_with_sample_count_and_plugin_block_size(
                1,
                fps,
                48_000,
                800,
                DEFAULT_PLUGIN_BLOCK_SIZE,
                &[],
            )
            .expect("custom export audio frame mixes");

        assert_eq!(preview.samples.len(), 801 * 2);
        assert_eq!(export.samples.len(), 800 * 2);
        assert_eq!(export.timeline_frame, 1);
        assert_eq!(export.start_sample, 800);
    }

    #[test]
    fn source_time_uses_speed_or_direct_time() {
        let mut plan = plan(1);
        assert_eq!(source_time_seconds(&plan, 30, 60.0), 2.0);
        plan.direct_mode = true;
        assert_eq!(source_time_seconds(&plan, 30, 60.0), 3.0);
    }

    #[test]
    fn a_missing_track_is_silent_without_aborting_the_frame() {
        let source = TimelineAudioSource {
            path: PathBuf::from("this-audio-must-not-exist.wav"),
            plan: plan(7),
        };
        let block = TimelineAudioMixer::default()
            .mix_frame_with_sample_count_and_plugin_block_size(
                0,
                60.0,
                48_000,
                800,
                DEFAULT_PLUGIN_BLOCK_SIZE,
                &[source],
            )
            .expect("valid frame timing still mixes");
        assert_eq!(block.samples.len(), 1_600);
        assert!(block.samples.iter().all(|sample| *sample == 0.0));
        assert_eq!(block.errors.len(), 1);
        assert!(block.meters[0].mixed);
    }

    #[test]
    fn decodes_and_mixes_consecutive_real_timeline_frames() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aviqtl-timeline-audio-{}-{nonce}.wav",
            std::process::id()
        ));
        let status = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:sample_rate=48000:duration=0.2",
                "-c:a",
                "pcm_s16le",
                "-y",
            ])
            .arg(&path)
            .status()
            .expect("ffmpeg process starts");
        assert!(status.success());

        let mut audio_plan = plan(8);
        audio_plan.source_path = Some(path.to_string_lossy().into_owned());
        audio_plan.source_start_time = 0.0;
        audio_plan.playback_speed = 1.0;
        let source = TimelineAudioSource {
            path: path.clone(),
            plan: audio_plan,
        };
        let mut mixer = TimelineAudioMixer::default();
        let first = mixer
            .mix_frame_with_sample_count_and_plugin_block_size(
                0,
                60.0,
                48_000,
                800,
                DEFAULT_PLUGIN_BLOCK_SIZE,
                std::slice::from_ref(&source),
            )
            .expect("first timeline frame mixes");
        let second = mixer
            .mix_frame_with_sample_count_and_plugin_block_size(
                1,
                60.0,
                48_000,
                800,
                DEFAULT_PLUGIN_BLOCK_SIZE,
                &[source],
            )
            .expect("second timeline frame mixes");
        assert_eq!(first.samples.len(), 1_600);
        assert_eq!(second.start_sample, 800);
        assert!(first.errors.is_empty());
        assert!(second.errors.is_empty());
        assert!(first.samples.iter().any(|sample| sample.abs() > 0.01));
        assert!(second.samples.iter().any(|sample| sample.abs() > 0.01));

        std::fs::remove_file(path).expect("generated audio removes");
    }
}
