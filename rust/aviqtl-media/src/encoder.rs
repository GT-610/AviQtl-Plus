use crate::VideoFrame;
use ffmpeg::codec;
use ffmpeg::codec::capabilities::Capabilities;
use ffmpeg::format::{self, Pixel, Sample, sample::Type as SampleType};
use ffmpeg::software::resampling::Context as ResamplingContext;
use ffmpeg::software::scaling::{context::Context as ScalingContext, flag::Flags};
use ffmpeg::util::frame::audio::Audio as AudioFrame;
use ffmpeg::util::frame::video::Video;
use ffmpeg::{ChannelLayout, ChannelLayoutMask, Dictionary, Packet, Rational};
use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, ImageFormat};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

const VIDEO_CODEC_CANDIDATES: [&str; 14] = [
    "libx264",
    "h264_nvenc",
    "h264_amf",
    "h264_qsv",
    "h264_vaapi",
    "libx265",
    "hevc_nvenc",
    "hevc_amf",
    "hevc_qsv",
    "hevc_vaapi",
    "libaom-av1",
    "av1_nvenc",
    "av1_amf",
    "av1_vaapi",
];
const AUDIO_CODEC_CANDIDATES: [&str; 5] = ["aac", "libopus", "libmp3lame", "flac", "pcm_s16le"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StillImageFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoEncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps_num: i32,
    pub fps_den: i32,
    pub bitrate: usize,
    pub crf: Option<i32>,
    pub gop_size: Option<u32>,
    pub codec_name: String,
    pub audio_codec_name: String,
    pub audio_bitrate: usize,
    pub sample_rate: u32,
    pub output_path: PathBuf,
    pub preset: String,
    pub profile: String,
}

#[derive(Debug)]
pub enum EncodeError {
    InvalidFrame,
    InvalidConfiguration,
    CodecUnavailable(String),
    Ffmpeg(ffmpeg::Error),
    Image(image::ImageError),
    Io(std::io::Error),
}

impl Display for EncodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFrame => formatter.write_str("invalid RGBA frame dimensions"),
            Self::InvalidConfiguration => formatter.write_str("invalid encoder configuration"),
            Self::CodecUnavailable(codec) => write!(formatter, "encoder is unavailable: {codec}"),
            Self::Ffmpeg(error) => write!(formatter, "FFmpeg: {error}"),
            Self::Image(error) => write!(formatter, "image encode: {error}"),
            Self::Io(error) => write!(formatter, "output I/O: {error}"),
        }
    }
}

impl Error for EncodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Ffmpeg(error) => Some(error),
            Self::Image(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::InvalidFrame | Self::InvalidConfiguration | Self::CodecUnavailable(_) => None,
        }
    }
}

impl From<ffmpeg::Error> for EncodeError {
    fn from(error: ffmpeg::Error) -> Self {
        Self::Ffmpeg(error)
    }
}

impl From<image::ImageError> for EncodeError {
    fn from(error: image::ImageError) -> Self {
        Self::Image(error)
    }
}

impl From<std::io::Error> for EncodeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn available_video_encoders() -> Vec<&'static str> {
    if ffmpeg::init().is_err() {
        return Vec::new();
    }
    VIDEO_CODEC_CANDIDATES
        .into_iter()
        .filter(|name| codec::encoder::find_by_name(name).is_some())
        .collect()
}

pub fn available_audio_encoders() -> Vec<&'static str> {
    if ffmpeg::init().is_err() {
        return Vec::new();
    }
    AUDIO_CODEC_CANDIDATES
        .into_iter()
        .filter(|name| codec::encoder::find_by_name(name).is_some())
        .collect()
}

pub fn save_still_image(
    path: &Path,
    frame: &VideoFrame,
    format: StillImageFormat,
    quality: u8,
) -> Result<(), EncodeError> {
    validate_frame(frame)?;
    match format {
        StillImageFormat::Png => image::save_buffer_with_format(
            path,
            &frame.rgba,
            frame.width,
            frame.height,
            ColorType::Rgba8,
            ImageFormat::Png,
        )?,
        StillImageFormat::Jpeg => {
            let mut rgb = Vec::with_capacity(frame.width as usize * frame.height as usize * 3);
            for pixel in frame.rgba.as_chunks::<4>().0 {
                rgb.extend_from_slice(&pixel[..3]);
            }
            let output = BufWriter::new(File::create(path)?);
            JpegEncoder::new_with_quality(output, quality.clamp(1, 100)).encode(
                &rgb,
                frame.width,
                frame.height,
                ColorType::Rgb8.into(),
            )?;
        }
    }
    Ok(())
}

pub struct VideoEncoder {
    output: format::context::Output,
    video: codec::encoder::Video,
    video_stream: usize,
    video_time_base: Rational,
    output_width: u32,
    output_height: u32,
    rgba_frame: Video,
    encoded_frame: Video,
    scaler: ScalingContext,
    audio: AudioEncoder,
    finished: bool,
}

impl VideoEncoder {
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate_config(config)?;
        ffmpeg::init()?;
        let video_codec = codec::encoder::find_by_name(&config.codec_name)
            .ok_or_else(|| EncodeError::CodecUnavailable(config.codec_name.clone()))?;
        let typed_video_codec = video_codec
            .video()
            .ok_or_else(|| EncodeError::CodecUnavailable(config.codec_name.clone()))?;
        let audio_codec = codec::encoder::find_by_name(&config.audio_codec_name)
            .or_else(|| codec::encoder::find_by_name("aac"))
            .ok_or_else(|| EncodeError::CodecUnavailable(config.audio_codec_name.clone()))?;
        let typed_audio_codec = audio_codec
            .audio()
            .ok_or_else(|| EncodeError::CodecUnavailable(config.audio_codec_name.clone()))?;
        let mut output = format::output(&config.output_path)?;
        let global_header = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);
        let video_time_base = Rational(config.fps_den, config.fps_num);
        let frame_rate = Rational(config.fps_num, config.fps_den);
        let pixel_format = choose_pixel_format(typed_video_codec);

        let mut video_context = codec::Context::new_with_codec(video_codec)
            .encoder()
            .video()?;
        video_context.set_width(config.width);
        video_context.set_height(config.height);
        video_context.set_format(pixel_format);
        video_context.set_time_base(video_time_base);
        video_context.set_frame_rate(Some(frame_rate));
        video_context.set_bit_rate(config.bitrate);
        if let Some(gop_size) = config.gop_size.filter(|value| *value > 0) {
            video_context.set_gop(gop_size);
        }
        if global_header {
            video_context.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let mut options = Dictionary::new();
        if let Some(crf) = config.crf {
            options.set("crf", crf.to_string());
            options.set("qp", crf.to_string());
        }
        if !config.preset.is_empty() {
            options.set("preset", &config.preset);
        }
        if !config.profile.is_empty() {
            options.set("profile", &config.profile);
        }
        let video = video_context.open_as_with(typed_video_codec, options)?;
        let video_stream = {
            let mut stream = output.add_stream(typed_video_codec)?;
            let index = stream.index();
            stream.set_time_base(video_time_base);
            stream.set_rate(frame_rate);
            stream.set_avg_frame_rate(frame_rate);
            stream.copy_parameters_from_context(&video);
            index
        };
        let audio = AudioEncoder::new(
            &mut output,
            audio_codec,
            typed_audio_codec,
            config.sample_rate,
            config.audio_bitrate,
            global_header,
        )?;
        output.write_header()?;

        Ok(Self {
            output,
            video,
            video_stream,
            video_time_base,
            output_width: config.width,
            output_height: config.height,
            rgba_frame: Video::new(Pixel::RGBA, config.width, config.height),
            encoded_frame: Video::new(pixel_format, config.width, config.height),
            scaler: ScalingContext::get(
                Pixel::RGBA,
                config.width,
                config.height,
                pixel_format,
                config.width,
                config.height,
                Flags::BILINEAR,
            )?,
            audio,
            finished: false,
        })
    }

    pub fn push_frame(&mut self, frame: &VideoFrame, pts: i64) -> Result<(), EncodeError> {
        validate_frame(frame)?;
        if frame.width != self.rgba_frame.width() || frame.height != self.rgba_frame.height() {
            self.rgba_frame = Video::new(Pixel::RGBA, frame.width, frame.height);
            self.scaler = ScalingContext::get(
                Pixel::RGBA,
                frame.width,
                frame.height,
                self.encoded_frame.format(),
                self.output_width,
                self.output_height,
                Flags::BILINEAR,
            )?;
        }
        copy_rgba_to_video_frame(frame, &mut self.rgba_frame);
        self.scaler.run(&self.rgba_frame, &mut self.encoded_frame)?;
        self.encoded_frame.set_pts(Some(pts));
        self.video.send_frame(&self.encoded_frame)?;
        drain_video_packets(
            &mut self.video,
            &mut self.output,
            self.video_stream,
            self.video_time_base,
        )
    }

    pub fn push_audio(&mut self, samples: &[f32]) -> Result<(), EncodeError> {
        self.audio.push(samples, &mut self.output)
    }

    pub fn finish(mut self) -> Result<(), EncodeError> {
        self.finish_inner()
    }

    fn finish_inner(&mut self) -> Result<(), EncodeError> {
        if self.finished {
            return Ok(());
        }
        self.audio.finish(&mut self.output)?;
        self.video.send_eof()?;
        drain_video_packets(
            &mut self.video,
            &mut self.output,
            self.video_stream,
            self.video_time_base,
        )?;
        self.output.write_trailer()?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        let _ = self.finish_inner();
    }
}

struct AudioEncoder {
    encoder: codec::encoder::Audio,
    stream: usize,
    time_base: Rational,
    resampler: ResamplingContext,
    pending: VecDeque<f32>,
    next_pts: i64,
    frame_size: usize,
    variable_frame_size: bool,
    sample_rate: u32,
    sample_format: Sample,
}

impl AudioEncoder {
    fn new(
        output: &mut format::context::Output,
        untyped_codec: codec::Codec,
        codec: codec::Audio,
        sample_rate: u32,
        bitrate: usize,
        global_header: bool,
    ) -> Result<Self, EncodeError> {
        let sample_format = choose_sample_format(codec);
        let time_base = Rational(1, sample_rate as i32);
        let mut context = codec::Context::new_with_codec(untyped_codec)
            .encoder()
            .audio()?;
        context.set_rate(sample_rate as i32);
        context.set_ch_layout(ChannelLayout::STEREO);
        context.set_format(sample_format);
        context.set_time_base(time_base);
        context.set_bit_rate(bitrate);
        if global_header {
            context.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let encoder = context.open_as(codec)?;
        let stream = {
            let mut stream = output.add_stream(codec)?;
            let index = stream.index();
            stream.set_time_base(time_base);
            stream.copy_parameters_from_context(&encoder);
            index
        };
        let frame_size = encoder.frame_size().max(1_024) as usize;
        let variable_frame_size = codec
            .capabilities()
            .contains(Capabilities::VARIABLE_FRAME_SIZE);
        Ok(Self {
            encoder,
            stream,
            time_base,
            resampler: ResamplingContext::get2(
                Sample::F32(SampleType::Packed),
                ChannelLayout::STEREO,
                sample_rate,
                sample_format,
                ChannelLayout::STEREO,
                sample_rate,
            )?,
            pending: VecDeque::new(),
            next_pts: 0,
            frame_size,
            variable_frame_size,
            sample_rate,
            sample_format,
        })
    }

    fn push(
        &mut self,
        samples: &[f32],
        output: &mut format::context::Output,
    ) -> Result<(), EncodeError> {
        self.pending.extend(
            samples
                .iter()
                .map(|sample| if sample.is_finite() { *sample } else { 0.0 }),
        );
        while self.pending.len() >= self.frame_size * 2 {
            self.encode_pending(self.frame_size, output)?;
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut format::context::Output) -> Result<(), EncodeError> {
        if !self.pending.is_empty() {
            let actual_frames = self.pending.len() / 2;
            let frame_count = if self.variable_frame_size {
                actual_frames
            } else {
                self.frame_size
            };
            while self.pending.len() < frame_count * 2 {
                self.pending.push_back(0.0);
            }
            self.encode_pending(frame_count, output)?;
        }
        self.encoder.send_eof()?;
        self.drain(output)
    }

    fn encode_pending(
        &mut self,
        frame_count: usize,
        output: &mut format::context::Output,
    ) -> Result<(), EncodeError> {
        let mut input = AudioFrame::new(
            Sample::F32(SampleType::Packed),
            frame_count,
            ChannelLayoutMask::STEREO,
        );
        input.set_rate(self.sample_rate);
        for frame in input.plane_mut::<(f32, f32)>(0) {
            frame.0 = self.pending.pop_front().unwrap_or(0.0);
            frame.1 = self.pending.pop_front().unwrap_or(0.0);
        }
        let mut converted =
            AudioFrame::new(self.sample_format, frame_count, ChannelLayoutMask::STEREO);
        converted.set_rate(self.sample_rate);
        self.resampler.run(&input, &mut converted)?;
        converted.set_pts(Some(self.next_pts));
        self.next_pts = self.next_pts.saturating_add(converted.samples() as i64);
        self.encoder.send_frame(&converted)?;
        self.drain(output)
    }

    fn drain(&mut self, output: &mut format::context::Output) -> Result<(), EncodeError> {
        let output_time_base = output
            .stream(self.stream)
            .expect("audio stream remains present")
            .time_base();
        let mut packet = Packet::empty();
        while self.encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(self.stream);
            packet.rescale_ts(self.time_base, output_time_base);
            packet.write_interleaved(output)?;
        }
        Ok(())
    }
}

fn validate_config(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if config.width == 0
        || config.height == 0
        || config.fps_num <= 0
        || config.fps_den <= 0
        || config.sample_rate == 0
        || config.output_path.as_os_str().is_empty()
    {
        return Err(EncodeError::InvalidConfiguration);
    }
    Ok(())
}

fn validate_frame(frame: &VideoFrame) -> Result<(), EncodeError> {
    let expected = frame.width as usize * frame.height as usize * 4;
    if frame.width == 0 || frame.height == 0 || frame.rgba.len() != expected {
        return Err(EncodeError::InvalidFrame);
    }
    Ok(())
}

fn choose_pixel_format(codec: codec::Video) -> Pixel {
    [Pixel::YUV420P, Pixel::YUV420P10LE, Pixel::NV12]
        .into_iter()
        .find(|format| codec.supports_format(*format))
        .unwrap_or(Pixel::YUV420P)
}

fn choose_sample_format(codec: codec::Audio) -> Sample {
    [
        Sample::F32(SampleType::Planar),
        Sample::F32(SampleType::Packed),
        Sample::I16(SampleType::Planar),
        Sample::I16(SampleType::Packed),
        Sample::I32(SampleType::Planar),
        Sample::I32(SampleType::Packed),
    ]
    .into_iter()
    .find(|format| codec.supports_format(*format))
    .unwrap_or(Sample::F32(SampleType::Planar))
}

fn copy_rgba_to_video_frame(source: &VideoFrame, destination: &mut Video) {
    let row_bytes = source.width as usize * 4;
    let stride = destination.stride(0);
    let destination = destination.data_mut(0);
    for (source_row, destination_row) in source
        .rgba
        .chunks_exact(row_bytes)
        .zip(destination.chunks_mut(stride))
    {
        destination_row[..row_bytes].copy_from_slice(source_row);
    }
}

fn drain_video_packets(
    encoder: &mut codec::encoder::Video,
    output: &mut format::context::Output,
    stream_index: usize,
    input_time_base: Rational,
) -> Result<(), EncodeError> {
    let output_time_base = output
        .stream(stream_index)
        .expect("video stream remains present")
        .time_base();
    let mut packet = Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(stream_index);
        packet.rescale_ts(input_time_base, output_time_base);
        packet.write_interleaved(output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(extension: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "aviqtl-encode-{}-{nonce}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn saves_png_and_quality_controlled_jpeg_frames() {
        let frame = VideoFrame {
            width: 2,
            height: 2,
            rgba: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
            timestamp_seconds: 0.0,
        };
        for (format, extension) in [
            (StillImageFormat::Png, "png"),
            (StillImageFormat::Jpeg, "jpg"),
        ] {
            let path = temporary_path(extension);
            save_still_image(&path, &frame, format, 91).expect("image saves");
            assert!(std::fs::metadata(&path).expect("image exists").len() > 0);
            std::fs::remove_file(path).expect("generated image removes");
        }
    }

    #[test]
    fn rejects_malformed_rgba_frames() {
        let path = temporary_path("png");
        let error = save_still_image(
            &path,
            &VideoFrame {
                width: 2,
                height: 2,
                rgba: vec![0; 3],
                timestamp_seconds: 0.0,
            },
            StillImageFormat::Png,
            95,
        )
        .expect_err("short buffer is rejected");
        assert!(matches!(error, EncodeError::InvalidFrame));
    }

    #[test]
    #[ignore = "requires ffmpeg software codecs (libx264, aac)"]
    fn encodes_real_video_and_audio_when_software_codecs_are_available() {
        if !available_video_encoders().contains(&"libx264")
            || !available_audio_encoders().contains(&"aac")
        {
            panic!("libx264/aac are required for this test; run with -- --ignored where available");
        }
        let path = temporary_path("mp4");
        let mut encoder = VideoEncoder::open(&VideoEncoderConfig {
            width: 64,
            height: 64,
            fps_num: 30,
            fps_den: 1,
            bitrate: 1_000_000,
            crf: Some(28),
            gop_size: None,
            codec_name: "libx264".to_owned(),
            audio_codec_name: "aac".to_owned(),
            audio_bitrate: 96_000,
            sample_rate: 48_000,
            output_path: path.clone(),
            preset: "ultrafast".to_owned(),
            profile: String::new(),
        })
        .expect("software encoder opens");
        for pts in 0..3 {
            let color = (pts * 80) as u8;
            encoder
                .push_frame(
                    &VideoFrame {
                        width: 64,
                        height: 64,
                        rgba: [color, 40, 180, 255].repeat(64 * 64),
                        timestamp_seconds: f64::from(pts) / 30.0,
                    },
                    i64::from(pts),
                )
                .expect("video frame encodes");
            encoder
                .push_audio(&vec![0.0; 1_600 * 2])
                .expect("audio frame encodes");
        }
        encoder.finish().expect("container finalizes");
        assert!(std::fs::metadata(&path).expect("video exists").len() > 0);
        std::fs::remove_file(path).expect("generated video removes");
    }
}
