//! Rust-owned media decoding for the egui/wgpu application.

mod encoder;

pub use encoder::{
    EncodeError, StillImageFormat, VideoEncoder, VideoEncoderConfig, available_audio_encoders,
    available_video_encoders, save_still_image,
};

use ffmpeg::format::Pixel;
use ffmpeg::format::{Sample, sample::Type as SampleType};
use ffmpeg::media::Type;
use ffmpeg::software::resampling::Context as ResamplingContext;
use ffmpeg::software::scaling::{context::Context as ScalingContext, flag::Flags};
use ffmpeg::util::frame::audio::Audio as AudioFrame;
use ffmpeg::util::frame::video::Video;
use ffmpeg::{ChannelLayout, ChannelLayoutMask};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

const SEEK_THRESHOLD_SECONDS: f64 = 2.0;
const MAX_CACHED_FRAMES: usize = 12;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const AUDIO_CHUNK_SECONDS: u32 = 4;
const MAX_CACHED_AUDIO_CHUNKS: usize = 10;

/// A tightly packed RGBA frame ready for upload to a wgpu texture.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub timestamp_seconds: f64,
}

/// Media errors presented by the application boundary.
#[derive(Debug)]
pub enum MediaError {
    Ffmpeg(ffmpeg::Error),
    Image(image::ImageError),
    Io(std::io::Error),
    MissingVideoStream,
    MissingAudioStream,
    EndOfStream,
    InvalidDimensions,
    InvalidSampleRate,
}

impl Display for MediaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ffmpeg(error) => write!(formatter, "FFmpeg: {error}"),
            Self::Image(error) => write!(formatter, "image decode: {error}"),
            Self::Io(error) => write!(formatter, "media I/O: {error}"),
            Self::MissingVideoStream => formatter.write_str("media has no video stream"),
            Self::MissingAudioStream => formatter.write_str("media has no audio stream"),
            Self::EndOfStream => {
                formatter.write_str("requested frame is past the end of the media")
            }
            Self::InvalidDimensions => formatter.write_str("decoded media dimensions are invalid"),
            Self::InvalidSampleRate => formatter.write_str("audio sample rate must be positive"),
        }
    }
}

impl Error for MediaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Ffmpeg(error) => Some(error),
            Self::Image(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::MissingVideoStream
            | Self::MissingAudioStream
            | Self::EndOfStream
            | Self::InvalidDimensions
            | Self::InvalidSampleRate => None,
        }
    }
}

impl From<ffmpeg::Error> for MediaError {
    fn from(error: ffmpeg::Error) -> Self {
        Self::Ffmpeg(error)
    }
}

impl From<image::ImageError> for MediaError {
    fn from(error: image::ImageError) -> Self {
        Self::Image(error)
    }
}

impl From<std::io::Error> for MediaError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Loads a still image and converts it to tightly packed RGBA8 pixels.
pub fn decode_image(path: &Path) -> Result<VideoFrame, MediaError> {
    let image = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    validate_dimensions(width, height, rgba.as_raw().len())?;
    Ok(VideoFrame {
        width,
        height,
        rgba: rgba.into_raw(),
        timestamp_seconds: 0.0,
    })
}

/// Interleaved stereo float32 samples at the decoder's configured output rate.
/// The rate is available from `AudioDecoder::sample_rate`; decoded blocks do
/// not repeat it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSamples {
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaStreamKind {
    Audio,
    Video,
}

/// Returns the best available duration for the requested media stream.
pub fn media_duration_seconds(
    path: &Path,
    stream_kind: MediaStreamKind,
) -> Result<Option<f64>, MediaError> {
    ffmpeg::init()?;
    let input = ffmpeg::format::input(path)?;
    let (media_type, missing_error) = match stream_kind {
        MediaStreamKind::Audio => (Type::Audio, MediaError::MissingAudioStream),
        MediaStreamKind::Video => (Type::Video, MediaError::MissingVideoStream),
    };
    let stream = input.streams().best(media_type).ok_or(missing_error)?;
    let stream_duration = stream.duration();
    if stream_duration > 0 {
        let time_base = stream.time_base();
        let seconds = stream_duration as f64 * f64::from(time_base.numerator())
            / f64::from(time_base.denominator());
        if seconds.is_finite() && seconds > 0.0 {
            return Ok(Some(seconds));
        }
    }
    let container_duration = input.duration();
    if container_duration > 0 {
        let seconds = container_duration as f64 / 1_000_000.0;
        if seconds.is_finite() && seconds > 0.0 {
            return Ok(Some(seconds));
        }
    }
    Ok(None)
}

struct AudioChunk {
    index: i64,
    samples: Vec<f32>,
}

/// Stateful, seekable audio decoder with a bounded four-second PCM cache.
pub struct AudioDecoder {
    input: ffmpeg::format::context::Input,
    stream_index: usize,
    time_base: f64,
    stream_start_seconds: f64,
    decoder: ffmpeg::decoder::Audio,
    sample_rate: u32,
    cache: VecDeque<AudioChunk>,
}

impl AudioDecoder {
    pub fn open(path: &Path, sample_rate: u32) -> Result<Self, MediaError> {
        if sample_rate == 0 {
            return Err(MediaError::InvalidSampleRate);
        }
        ffmpeg::init()?;
        let input = ffmpeg::format::input(path)?;
        let stream = input
            .streams()
            .best(Type::Audio)
            .ok_or(MediaError::MissingAudioStream)?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let time_base = f64::from(time_base.numerator()) / f64::from(time_base.denominator());
        let stream_start = stream.start_time();
        let stream_start_seconds = if stream_start == i64::MIN {
            0.0
        } else {
            stream_start as f64 * time_base
        };
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().audio()?;
        Ok(Self {
            input,
            stream_index,
            time_base,
            stream_start_seconds,
            decoder,
            sample_rate,
            cache: VecDeque::new(),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Decodes `frame_count` stereo sample frames beginning at `start_seconds`.
    /// Regions before the first decoded packet or beyond end-of-stream remain silent.
    pub fn decode_range(
        &mut self,
        start_seconds: f64,
        frame_count: usize,
    ) -> Result<AudioSamples, MediaError> {
        let start_sample = seconds_to_sample(start_seconds.max(0.0), self.sample_rate);
        let chunk_frames = audio_chunk_frames(self.sample_rate);
        let mut output = vec![0.0; frame_count.saturating_mul(2)];
        let mut written_frames = 0;
        while written_frames < frame_count {
            let absolute_frame = start_sample.saturating_add(written_frames as i64);
            let chunk_index = absolute_frame.div_euclid(chunk_frames as i64);
            self.ensure_chunk(chunk_index)?;
            let chunk = self
                .cache
                .iter()
                .find(|chunk| chunk.index == chunk_index)
                .expect("decoded audio chunk was cached above");
            let offset = absolute_frame.rem_euclid(chunk_frames as i64) as usize;
            let available = chunk_frames.saturating_sub(offset);
            let copy_frames = available.min(frame_count - written_frames);
            let source_start = offset * 2;
            let source_end = source_start + copy_frames * 2;
            let destination_start = written_frames * 2;
            let destination_end = destination_start + copy_frames * 2;
            output[destination_start..destination_end]
                .copy_from_slice(&chunk.samples[source_start..source_end]);
            written_frames += copy_frames;
        }
        Ok(AudioSamples {
            samples: output,
        })
    }

    fn ensure_chunk(&mut self, index: i64) -> Result<(), MediaError> {
        if let Some(position) = self.cache.iter().position(|chunk| chunk.index == index) {
            let chunk = self
                .cache
                .remove(position)
                .expect("cached audio chunk position remains valid");
            self.cache.push_back(chunk);
            return Ok(());
        }
        let samples = self.decode_chunk(index)?;
        self.cache.push_back(AudioChunk { index, samples });
        while self.cache.len() > MAX_CACHED_AUDIO_CHUNKS {
            self.cache.pop_front();
        }
        Ok(())
    }

    fn decode_chunk(&mut self, index: i64) -> Result<Vec<f32>, MediaError> {
        let chunk_frames = audio_chunk_frames(self.sample_rate);
        let chunk_start_frame = index.saturating_mul(chunk_frames as i64);
        let chunk_start_seconds = chunk_start_frame as f64 / f64::from(self.sample_rate);
        let chunk_end_frame = chunk_start_frame.saturating_add(chunk_frames as i64);
        self.seek_to(chunk_start_seconds)?;
        let mut resampler = None;
        let mut output = vec![0.0; chunk_frames * 2];
        let mut fallback_frame = chunk_start_frame;
        let mut reached_end = false;

        while !reached_end {
            let (stream, packet) = match self.input.packets().next() {
                Some(Ok(packet)) => packet,
                Some(Err(error)) => return Err(error.into()),
                None => {
                    self.decoder.send_eof()?;
                    self.receive_audio_frames(
                        &mut resampler,
                        chunk_start_frame,
                        chunk_end_frame,
                        &mut fallback_frame,
                        &mut output,
                    )?;
                    break;
                }
            };
            if stream.index() != self.stream_index {
                continue;
            }
            self.decoder.send_packet(&packet)?;
            reached_end = self.receive_audio_frames(
                &mut resampler,
                chunk_start_frame,
                chunk_end_frame,
                &mut fallback_frame,
                &mut output,
            )?;
        }
        Ok(output)
    }

    fn receive_audio_frames(
        &mut self,
        resampler: &mut Option<ResamplingContext>,
        chunk_start_frame: i64,
        chunk_end_frame: i64,
        fallback_frame: &mut i64,
        output: &mut [f32],
    ) -> Result<bool, MediaError> {
        let mut decoded = AudioFrame::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            normalize_audio_layout(&mut decoded);
            let source_start_frame = decoded.timestamp().map_or(*fallback_frame, |timestamp| {
                let seconds = timestamp as f64 * self.time_base - self.stream_start_seconds;
                seconds_to_sample(seconds.max(0.0), self.sample_rate)
            });
            if source_start_frame >= chunk_end_frame {
                return Ok(true);
            }
            if resampler.is_none() {
                *resampler = Some(create_audio_resampler(&decoded, self.sample_rate)?);
            }
            let resampler = resampler
                .as_mut()
                .expect("audio resampler was initialized above");
            let output_capacity = resampled_capacity(resampler, decoded.samples());
            let mut converted = AudioFrame::new(
                Sample::F32(SampleType::Packed),
                output_capacity,
                ChannelLayoutMask::STEREO,
            );
            resampler.run(&decoded, &mut converted)?;
            let samples = converted.plane::<(f32, f32)>(0);
            copy_audio_overlap(
                output,
                chunk_start_frame,
                chunk_end_frame,
                source_start_frame,
                samples,
            );
            *fallback_frame = source_start_frame.saturating_add(samples.len() as i64);
            if *fallback_frame >= chunk_end_frame {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn seek_to(&mut self, target_seconds: f64) -> Result<(), MediaError> {
        let absolute_seconds = target_seconds + self.stream_start_seconds;
        let timestamp = (absolute_seconds * 1_000_000.0)
            .round()
            .clamp(i64::MIN as f64, i64::MAX as f64) as i64;
        self.input.seek(timestamp, ..=timestamp)?;
        self.decoder.flush();
        Ok(())
    }
}

fn normalize_audio_layout(frame: &mut AudioFrame) {
    let layout = frame.ch_layout();
    if layout.mask().is_some() {
        return;
    }
    let channels = layout.channels();
    drop(layout);
    frame.set_ch_layout(ChannelLayout::default_for_channels(channels));
}

fn create_audio_resampler(
    frame: &AudioFrame,
    sample_rate: u32,
) -> Result<ResamplingContext, MediaError> {
    let frame_layout = frame.ch_layout();
    let source_layout = if frame_layout.mask().is_some() {
        frame_layout.clone()
    } else {
        ChannelLayout::default_for_channels(frame_layout.channels())
    };
    Ok(ResamplingContext::get2(
        frame.format(),
        source_layout,
        frame.rate(),
        Sample::F32(SampleType::Packed),
        ChannelLayout::STEREO,
        sample_rate,
    )?)
}

fn audio_chunk_frames(sample_rate: u32) -> usize {
    sample_rate as usize * AUDIO_CHUNK_SECONDS as usize
}

fn resampled_capacity(resampler: &ResamplingContext, input_samples: usize) -> usize {
    let input_rate = u64::from(resampler.input().rate.max(1));
    let output_rate = u64::from(resampler.output().rate);
    let converted = (input_samples as u64)
        .saturating_mul(output_rate)
        .saturating_add(input_rate - 1)
        / input_rate;
    let delayed = resampler
        .delay()
        .map_or(0, |delay| delay.output.max(0) as u64);
    converted
        .saturating_add(delayed)
        .saturating_add(32)
        .min(usize::MAX as u64) as usize
}

fn seconds_to_sample(seconds: f64, sample_rate: u32) -> i64 {
    (seconds * f64::from(sample_rate))
        .round()
        .clamp(0.0, i64::MAX as f64) as i64
}

fn copy_audio_overlap(
    destination: &mut [f32],
    destination_start_frame: i64,
    destination_end_frame: i64,
    source_start_frame: i64,
    source: &[(f32, f32)],
) {
    let source_end_frame = source_start_frame.saturating_add(source.len() as i64);
    let overlap_start = destination_start_frame.max(source_start_frame);
    let overlap_end = destination_end_frame.min(source_end_frame);
    if overlap_start >= overlap_end {
        return;
    }
    let source_offset = (overlap_start - source_start_frame) as usize;
    let destination_offset = (overlap_start - destination_start_frame) as usize;
    let frame_count = (overlap_end - overlap_start) as usize;
    for index in 0..frame_count {
        let (left, right) = source[source_offset + index];
        destination[(destination_offset + index) * 2] = left;
        destination[(destination_offset + index) * 2 + 1] = right;
    }
}

/// Stateful video decoder. Forward playback reuses the demuxer and codec; backward seeks reopen
/// the source until indexed seeking is added to this Rust boundary.
pub struct VideoDecoder {
    path: PathBuf,
    input: ffmpeg::format::context::Input,
    stream_index: usize,
    time_base: f64,
    stream_start_seconds: f64,
    source_fps: f64,
    decoder: ffmpeg::decoder::Video,
    scaler: ScalingContext,
    decoded_seconds: f64,
    reached_eof: bool,
    last_frame: Option<VideoFrame>,
    cache: VecDeque<(i64, VideoFrame)>,
    cache_bytes: usize,
}

impl VideoDecoder {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        ffmpeg::init()?;
        let input = ffmpeg::format::input(path)?;
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or(MediaError::MissingVideoStream)?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let time_base = f64::from(time_base.numerator()) / f64::from(time_base.denominator());
        let stream_start = stream.start_time();
        let stream_start_seconds = if stream_start == i64::MIN {
            0.0
        } else {
            stream_start as f64 * time_base
        };
        let frame_rate = stream.avg_frame_rate();
        let source_fps = if frame_rate.denominator() == 0 {
            0.0
        } else {
            f64::from(frame_rate.numerator()) / f64::from(frame_rate.denominator())
        };
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().video()?;
        let scaler = ScalingContext::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGBA,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            input,
            stream_index,
            time_base,
            stream_start_seconds,
            source_fps,
            decoder,
            scaler,
            decoded_seconds: -1.0,
            reached_eof: false,
            last_frame: None,
            cache: VecDeque::new(),
            cache_bytes: 0,
        })
    }

    /// Returns the average source frame rate reported by the video stream.
    pub fn source_fps(&self) -> f64 {
        self.source_fps
    }

    pub fn decode_at(&mut self, timestamp_seconds: f64) -> Result<VideoFrame, MediaError> {
        let target = timestamp_seconds.max(0.0);
        let cache_key = timestamp_key(target);
        if let Some(frame) = self.cached_frame(cache_key) {
            return Ok(frame);
        }
        if (target - self.decoded_seconds).abs() <= f64::EPSILON
            && let Some(frame) = &self.last_frame
        {
            return Ok(frame.clone());
        }
        if target + f64::EPSILON < self.decoded_seconds
            || target - self.decoded_seconds > SEEK_THRESHOLD_SECONDS
        {
            self.seek_to(target)?;
        }

        while !self.reached_eof {
            let (stream, packet) = match self.input.packets().next() {
                Some(Ok(packet)) => packet,
                Some(Err(error)) => return Err(error.into()),
                None => {
                    self.decoder.send_eof()?;
                    self.reached_eof = true;
                    break;
                }
            };
            if stream.index() != self.stream_index {
                continue;
            }
            self.decoder.send_packet(&packet)?;
            if let Some(frame) = self.receive_until(target)? {
                self.remember_frame(cache_key, &frame);
                return Ok(frame);
            }
        }
        if let Some(frame) = self.receive_until(target)? {
            self.remember_frame(cache_key, &frame);
            return Ok(frame);
        }
        Err(MediaError::EndOfStream)
    }

    fn seek_to(&mut self, target: f64) -> Result<(), MediaError> {
        let absolute_seconds = target + self.stream_start_seconds;
        let timestamp = (absolute_seconds * 1_000_000.0)
            .round()
            .clamp(i64::MIN as f64, i64::MAX as f64) as i64;
        if self.input.seek(timestamp, ..=timestamp).is_err() {
            let cache = std::mem::take(&mut self.cache);
            let cache_bytes = self.cache_bytes;
            let mut reopened = match Self::open(&self.path) {
                Ok(reopened) => reopened,
                Err(error) => {
                    self.cache = cache;
                    return Err(error);
                }
            };
            reopened.cache = cache;
            reopened.cache_bytes = cache_bytes;
            *self = reopened;
            return Ok(());
        }
        self.decoder.flush();
        self.decoded_seconds = -1.0;
        self.reached_eof = false;
        self.last_frame = None;
        Ok(())
    }

    fn cached_frame(&mut self, key: i64) -> Option<VideoFrame> {
        let index = self
            .cache
            .iter()
            .position(|(candidate, _)| *candidate == key)?;
        let entry = self.cache.remove(index)?;
        let frame = entry.1.clone();
        self.cache.push_back(entry);
        Some(frame)
    }

    fn remember_frame(&mut self, key: i64, frame: &VideoFrame) {
        if self.cache.iter().any(|(candidate, _)| *candidate == key) {
            return;
        }
        self.cache_bytes = self.cache_bytes.saturating_add(frame.rgba.len());
        self.cache.push_back((key, frame.clone()));
        while self.cache.len() > 1
            && (self.cache.len() > MAX_CACHED_FRAMES || self.cache_bytes > MAX_CACHE_BYTES)
        {
            if let Some((_, removed)) = self.cache.pop_front() {
                self.cache_bytes = self.cache_bytes.saturating_sub(removed.rgba.len());
            }
        }
    }

    fn receive_until(&mut self, target: f64) -> Result<Option<VideoFrame>, MediaError> {
        let mut decoded = Video::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            let seconds = decoded
                .timestamp()
                .map_or(self.decoded_seconds.max(0.0), |timestamp| {
                    timestamp as f64 * self.time_base - self.stream_start_seconds
                })
                .max(0.0);
            self.decoded_seconds = seconds;
            if seconds + f64::EPSILON >= target {
                let frame = self.convert(&decoded, seconds)?;
                self.last_frame = Some(frame.clone());
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }

    fn convert(&mut self, decoded: &Video, seconds: f64) -> Result<VideoFrame, MediaError> {
        let mut rgba = Video::empty();
        self.scaler.run(decoded, &mut rgba)?;
        let width = rgba.width();
        let height = rgba.height();
        let row_bytes = width as usize * 4;
        let mut pixels = vec![0_u8; row_bytes * height as usize];
        for row in 0..height as usize {
            let source = row * rgba.stride(0);
            let destination = row * row_bytes;
            pixels[destination..destination + row_bytes]
                .copy_from_slice(&rgba.data(0)[source..source + row_bytes]);
        }
        validate_dimensions(width, height, pixels.len())?;
        Ok(VideoFrame {
            width,
            height,
            rgba: pixels,
            timestamp_seconds: seconds,
        })
    }
}

fn timestamp_key(seconds: f64) -> i64 {
    (seconds * 1_000_000.0)
        .round()
        .clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

fn validate_dimensions(width: u32, height: u32, byte_length: usize) -> Result<(), MediaError> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(byte_length) {
        Err(MediaError::InvalidDimensions)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn rejects_invalid_rgba_dimensions() {
        assert!(validate_dimensions(0, 1, 0).is_err());
        assert!(validate_dimensions(4, 4, 63).is_err());
        assert!(validate_dimensions(u32::MAX, u32::MAX, 0).is_err());
        assert!(validate_dimensions(4, 4, 64).is_ok());
    }

    #[test]
    fn missing_image_is_reported_without_panicking() {
        let path = Path::new("this-image-must-not-exist.aviqtl-test.png");
        assert!(matches!(decode_image(path), Err(MediaError::Io(_))));
    }

    #[test]
    fn audio_overlap_copies_interleaved_stereo_at_the_requested_offset() {
        let source = [
            (1.0, -1.0),
            (2.0, -2.0),
            (3.0, -3.0),
            (4.0, -4.0),
            (5.0, -5.0),
            (6.0, -6.0),
        ];
        let mut destination = vec![0.0; 8];
        copy_audio_overlap(&mut destination, 10, 14, 8, &source);
        assert_eq!(destination, [3.0, -3.0, 4.0, -4.0, 5.0, -5.0, 6.0, -6.0]);
    }

    #[test]
    fn rejects_zero_audio_sample_rate() {
        let path = Path::new("this-audio-must-not-open.aviqtl-test.wav");
        assert!(matches!(
            AudioDecoder::open(path, 0),
            Err(MediaError::InvalidSampleRate)
        ));
    }

    #[test]
    #[ignore = "requires an ffmpeg binary on PATH"]
    fn decodes_a_real_generated_video_when_ffmpeg_cli_is_available() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            panic!("ffmpeg is required for this test; run with -- --ignored on machines with ffmpeg");
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("aviqtl-media-{}-{nonce}.mkv", std::process::id()));
        let status = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=16x8:r=10:d=0.3",
                "-c:v",
                "ffv1",
                "-y",
            ])
            .arg(&path)
            .status()
            .expect("ffmpeg process starts");
        assert!(status.success());

        let mut decoder = VideoDecoder::open(&path).expect("generated video opens");
        assert!((decoder.source_fps() - 10.0).abs() < 1e-9);
        let first = decoder.decode_at(0.0).expect("first frame decodes");
        assert_eq!((first.width, first.height), (16, 8));
        assert_eq!(first.rgba.len(), 16 * 8 * 4);
        assert!(first.rgba[0] > 200);
        assert!(first.rgba[1] < 80);
        assert!(first.rgba[2] < 80);
        assert_eq!(decoder.decode_at(0.0).expect("frame is cached"), first);
        let later = decoder.decode_at(0.15).expect("later frame decodes");
        assert!(later.timestamp_seconds >= 0.1);
        let rewound = decoder.decode_at(0.0).expect("backward request seeks");
        assert_eq!((rewound.width, rewound.height), (16, 8));
        assert_eq!(
            decoder.decode_at(0.15).expect("recent frame is cached"),
            later
        );

        std::fs::remove_file(path).expect("generated video removes");
    }

    #[test]
    #[ignore = "requires an ffmpeg binary on PATH"]
    fn decodes_and_caches_a_real_audio_range_when_ffmpeg_cli_is_available() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            panic!("ffmpeg is required for this test; run with -- --ignored on machines with ffmpeg");
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("aviqtl-audio-{}-{nonce}.wav", std::process::id()));
        let status = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=24000:duration=0.3",
                "-c:a",
                "pcm_s16le",
                "-y",
            ])
            .arg(&path)
            .status()
            .expect("ffmpeg process starts");
        assert!(status.success());

        let duration = media_duration_seconds(&path, MediaStreamKind::Audio)
            .expect("generated audio duration probes")
            .expect("generated audio reports a duration");
        assert!((duration - 0.3).abs() < 0.02);

        let mut decoder = AudioDecoder::open(&path, 48_000).expect("generated audio opens");
        assert_eq!(decoder.sample_rate(), 48_000);
        let first = decoder
            .decode_range(0.05, 4_800)
            .expect("first audio range decodes");
        assert_eq!(first.samples.len() / 2, 4_800);
        assert!(first.samples.iter().any(|sample| sample.abs() > 0.01));
        assert_eq!(
            decoder
                .decode_range(0.05, 4_800)
                .expect("cached audio range decodes"),
            first
        );
        let tail = decoder
            .decode_range(0.25, 4_800)
            .expect("range crossing end-of-stream decodes");
        assert_eq!(tail.samples.len() / 2, 4_800);
        assert!(
            tail.samples[..2_000]
                .iter()
                .any(|sample| sample.abs() > 0.01)
        );
        assert!(tail.samples[6_000..].iter().all(|sample| *sample == 0.0));

        std::fs::remove_file(path).expect("generated audio removes");
    }
}
