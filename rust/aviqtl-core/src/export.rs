use crate::abi::{
    AviQtlExportAudioFramePlan, AviQtlExportImageSequencePlan, AviQtlExportImageSequenceRequest,
    AviQtlExportProgressPlan, AviQtlExportVideoDefaults, AviQtlExportVideoPlan,
    AviQtlExportVideoRequest, STATUS_INVALID_ARGUMENT, STATUS_OK, STATUS_OVERLAPPING_BUFFERS,
    ranges_overlap, slice_is_valid, utf8,
};
use std::path::PathBuf;

const CONFIGURATION_OK: u32 = 0;
const CONFIGURATION_MISSING_OUTPUT_PATH: u32 = 1;
const CONFIGURATION_INVALID_OUTPUT_SIZE: u32 = 2;
const CONFIGURATION_INVALID_FPS: u32 = 3;
const CONFIGURATION_INVALID_RANGE: u32 = 4;
const CONFIGURATION_PROJECT_FPS_MISMATCH: u32 = 5;

const IMAGE_FORMAT_PNG: u32 = 0;
const IMAGE_FORMAT_JPEG: u32 = 1;

const CODEC_BACKEND_SOFTWARE: u32 = 0;
const CODEC_BACKEND_CUDA: u32 = 1;
const CODEC_BACKEND_VAAPI: u32 = 2;
const CODEC_BACKEND_QSV: u32 = 3;
const CODEC_BACKEND_D3D11VA: u32 = 4;
const CODEC_BACKEND_DXVA2: u32 = 5;
const CODEC_BACKEND_VIDEOTOOLBOX: u32 = 6;
const CODEC_BACKEND_AMF: u32 = 7;

const FIXED_GOP_NONE: u32 = 0;
const FIXED_GOP_X264: u32 = 1;
const FIXED_GOP_X265: u32 = 2;
const FIXED_GOP_NVENC: u32 = 3;

const DEFAULT_VIDEO_CODEC: &str = "libx264";
const DEFAULT_AUDIO_CODEC: &str = "aac";
const MIN_ENCODER_QUEUE_TASKS: u128 = 2;
const MAX_ENCODER_QUEUE_TASKS: u128 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportConfigurationError {
    MissingOutputPath,
    InvalidOutputSize,
    InvalidFps,
    InvalidRange,
    ProjectFpsMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportImageFormat {
    Png,
    Jpeg,
}

impl ExportImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportCodecBackend {
    Software,
    Cuda,
    Vaapi,
    Qsv,
    D3d11va,
    Dxva2,
    VideoToolbox,
    Amf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFixedGopMode {
    None,
    X264,
    X265,
    Nvenc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoExportDefaults {
    pub width: i32,
    pub height: i32,
    pub fps_num: i32,
    pub fps_den: i32,
    pub bitrate: i64,
    pub crf: i32,
    pub gop_size: i32,
    pub audio_bitrate: i64,
    pub start_frame: i32,
    pub end_frame: i32,
    pub video_codec: &'static str,
    pub audio_codec: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoExportRequest {
    pub width: i32,
    pub height: i32,
    pub fps_num: i32,
    pub fps_den: i32,
    pub start_frame: i32,
    pub end_frame: i32,
    pub timeline_duration: i32,
    pub output_path: PathBuf,
    pub project_fps: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoExportPlan {
    pub start_frame: i32,
    pub end_frame: i32,
    pub total_frames: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSequenceExportRequest {
    pub start_frame: i32,
    pub end_frame: i32,
    pub timeline_duration: i32,
    pub configured_padding: i32,
    pub output_directory: PathBuf,
    pub format: ExportImageFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSequenceExportPlan {
    pub start_frame: i32,
    pub end_frame: i32,
    pub total_frames: i32,
    pub pad_digits: i32,
    pub format: ExportImageFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportAudioFramePlan {
    pub cumulative_samples: i64,
    pub samples_for_frame: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportProgressPlan {
    pub progress: i32,
    pub current_frame: i32,
    pub total_frames: i32,
    pub eta_seconds: i32,
    pub should_emit: bool,
}

pub fn video_export_defaults() -> VideoExportDefaults {
    let values = video_defaults();
    VideoExportDefaults {
        width: values.width,
        height: values.height,
        fps_num: values.fps_num,
        fps_den: values.fps_den,
        bitrate: values.bitrate,
        crf: values.crf,
        gop_size: values.gop_size,
        audio_bitrate: values.audio_bitrate,
        start_frame: values.start_frame,
        end_frame: values.end_frame,
        video_codec: DEFAULT_VIDEO_CODEC,
        audio_codec: DEFAULT_AUDIO_CODEC,
    }
}

pub fn plan_video_export(
    request: &VideoExportRequest,
) -> Result<VideoExportPlan, ExportConfigurationError> {
    let plan = plan_video(AviQtlExportVideoRequest {
        width: request.width,
        height: request.height,
        fps_num: request.fps_num,
        fps_den: request.fps_den,
        start_frame: request.start_frame,
        end_frame: request.end_frame,
        timeline_duration: request.timeline_duration,
        output_path_present: u32::from(!request.output_path.as_os_str().is_empty()),
        project_fps: request.project_fps,
    });
    configuration_result(plan.error)?;
    Ok(VideoExportPlan {
        start_frame: plan.start_frame,
        end_frame: plan.end_frame,
        total_frames: plan.total_frames,
    })
}

pub fn plan_image_sequence_export(
    request: &ImageSequenceExportRequest,
) -> Result<ImageSequenceExportPlan, ExportConfigurationError> {
    let format = match request.format {
        ExportImageFormat::Png => "PNG",
        ExportImageFormat::Jpeg => "JPEG",
    };
    let plan = plan_image_sequence(
        AviQtlExportImageSequenceRequest {
            start_frame: request.start_frame,
            end_frame: request.end_frame,
            timeline_duration: request.timeline_duration,
            configured_padding: request.configured_padding,
            output_path_present: u32::from(!request.output_directory.as_os_str().is_empty()),
        },
        format,
    );
    configuration_result(plan.error)?;
    Ok(ImageSequenceExportPlan {
        start_frame: plan.start_frame,
        end_frame: plan.end_frame,
        total_frames: plan.total_frames,
        pad_digits: plan.pad_digits,
        format: if plan.image_format == IMAGE_FORMAT_JPEG {
            ExportImageFormat::Jpeg
        } else {
            ExportImageFormat::Png
        },
    })
}

pub fn plan_export_audio_frame(
    frame_index: i32,
    sample_rate: i32,
    fps_num: i32,
    fps_den: i32,
) -> Option<ExportAudioFramePlan> {
    plan_audio_frame(frame_index, sample_rate, fps_num, fps_den).map(|plan| ExportAudioFramePlan {
        cumulative_samples: plan.cumulative_samples,
        samples_for_frame: plan.samples_for_frame,
    })
}

pub fn plan_export_progress(
    done: i32,
    total_frames: i32,
    interval: i32,
    elapsed_ms: i64,
) -> Option<ExportProgressPlan> {
    plan_progress(done, total_frames, interval, elapsed_ms).map(|plan| ExportProgressPlan {
        progress: plan.progress,
        current_frame: plan.current_frame,
        total_frames: plan.total_frames,
        eta_seconds: plan.eta_seconds,
        should_emit: plan.should_emit != 0,
    })
}

pub fn export_codec_backend(codec_name: &str) -> ExportCodecBackend {
    match codec_backend(codec_name) {
        CODEC_BACKEND_CUDA => ExportCodecBackend::Cuda,
        CODEC_BACKEND_VAAPI => ExportCodecBackend::Vaapi,
        CODEC_BACKEND_QSV => ExportCodecBackend::Qsv,
        CODEC_BACKEND_D3D11VA => ExportCodecBackend::D3d11va,
        CODEC_BACKEND_DXVA2 => ExportCodecBackend::Dxva2,
        CODEC_BACKEND_VIDEOTOOLBOX => ExportCodecBackend::VideoToolbox,
        CODEC_BACKEND_AMF => ExportCodecBackend::Amf,
        _ => ExportCodecBackend::Software,
    }
}

pub fn export_codec_fallback(codec_name: &str) -> Option<&'static str> {
    codec_fallback(codec_name)
}

pub fn export_fixed_gop_mode(codec_name: &str) -> ExportFixedGopMode {
    match fixed_gop_mode(codec_name) {
        FIXED_GOP_X264 => ExportFixedGopMode::X264,
        FIXED_GOP_X265 => ExportFixedGopMode::X265,
        FIXED_GOP_NVENC => ExportFixedGopMode::Nvenc,
        _ => ExportFixedGopMode::None,
    }
}

pub fn export_encoder_queue_size(width: i32, height: i32, budget_mb: i32) -> usize {
    encoder_queue_size(width, height, budget_mb)
}

fn configuration_result(error: u32) -> Result<(), ExportConfigurationError> {
    match error {
        CONFIGURATION_OK => Ok(()),
        CONFIGURATION_MISSING_OUTPUT_PATH => Err(ExportConfigurationError::MissingOutputPath),
        CONFIGURATION_INVALID_OUTPUT_SIZE => Err(ExportConfigurationError::InvalidOutputSize),
        CONFIGURATION_INVALID_FPS => Err(ExportConfigurationError::InvalidFps),
        CONFIGURATION_INVALID_RANGE => Err(ExportConfigurationError::InvalidRange),
        CONFIGURATION_PROJECT_FPS_MISMATCH => Err(ExportConfigurationError::ProjectFpsMismatch),
        _ => Err(ExportConfigurationError::InvalidFps),
    }
}

fn video_defaults() -> AviQtlExportVideoDefaults {
    AviQtlExportVideoDefaults {
        width: 1920,
        height: 1080,
        fps_num: 60_000,
        fps_den: 1_000,
        bitrate: 15_000_000,
        crf: -1,
        gop_size: 0,
        audio_bitrate: 192_000,
        start_frame: 0,
        end_frame: -1,
    }
}

fn plan_video(request: AviQtlExportVideoRequest) -> AviQtlExportVideoPlan {
    let end_frame = if request.end_frame >= 0 {
        request.end_frame
    } else {
        request.timeline_duration
    };
    let error = if request.output_path_present == 0 {
        CONFIGURATION_MISSING_OUTPUT_PATH
    } else if request.width <= 0 || request.height <= 0 {
        CONFIGURATION_INVALID_OUTPUT_SIZE
    } else if request.fps_num <= 0 || request.fps_den <= 0 {
        CONFIGURATION_INVALID_FPS
    } else if request.start_frame < 0 || end_frame <= request.start_frame {
        CONFIGURATION_INVALID_RANGE
    } else {
        let export_fps = f64::from(request.fps_num) / f64::from(request.fps_den);
        if !request.project_fps.is_finite()
            || request.project_fps <= 0.0
            || (export_fps - request.project_fps).abs() > 0.001
        {
            CONFIGURATION_PROJECT_FPS_MISMATCH
        } else {
            CONFIGURATION_OK
        }
    };

    AviQtlExportVideoPlan {
        start_frame: request.start_frame,
        end_frame,
        total_frames: if error == CONFIGURATION_OK {
            end_frame - request.start_frame
        } else {
            0
        },
        error,
    }
}

fn decimal_digits(value: i32) -> i32 {
    let mut value = value.max(0);
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

fn plan_image_sequence(
    request: AviQtlExportImageSequenceRequest,
    format: &str,
) -> AviQtlExportImageSequencePlan {
    let end_frame = if request.end_frame >= 0 {
        request.end_frame
    } else {
        request.timeline_duration
    };
    let error = if request.output_path_present == 0 {
        CONFIGURATION_MISSING_OUTPUT_PATH
    } else if request.start_frame < 0 || end_frame <= request.start_frame {
        CONFIGURATION_INVALID_RANGE
    } else {
        CONFIGURATION_OK
    };
    let configured_padding = request.configured_padding.clamp(2, 10);
    let pad_digits = configured_padding.max(decimal_digits(end_frame.saturating_sub(1)));

    AviQtlExportImageSequencePlan {
        start_frame: request.start_frame,
        end_frame,
        total_frames: if error == CONFIGURATION_OK {
            end_frame - request.start_frame
        } else {
            0
        },
        pad_digits,
        image_format: if format == "JPEG" {
            IMAGE_FORMAT_JPEG
        } else {
            IMAGE_FORMAT_PNG
        },
        error,
    }
}

fn cumulative_samples(
    frame_count: i32,
    sample_rate: i32,
    fps_num: i32,
    fps_den: i32,
) -> Option<i64> {
    let numerator = i128::from(frame_count)
        .checked_mul(i128::from(sample_rate))?
        .checked_mul(i128::from(fps_den))?;
    let value = numerator / i128::from(fps_num);
    i64::try_from(value).ok()
}

fn plan_audio_frame(
    frame_index: i32,
    sample_rate: i32,
    fps_num: i32,
    fps_den: i32,
) -> Option<AviQtlExportAudioFramePlan> {
    if frame_index < 0 || sample_rate <= 0 || fps_num <= 0 || fps_den <= 0 {
        return None;
    }
    let current = cumulative_samples(frame_index, sample_rate, fps_num, fps_den)?;
    let next = cumulative_samples(frame_index.checked_add(1)?, sample_rate, fps_num, fps_den)?;
    Some(AviQtlExportAudioFramePlan {
        cumulative_samples: next,
        samples_for_frame: i32::try_from(next.checked_sub(current)?).ok()?,
        reserved: 0,
    })
}

fn plan_progress(
    done: i32,
    total_frames: i32,
    interval: i32,
    elapsed_ms: i64,
) -> Option<AviQtlExportProgressPlan> {
    if done <= 0 || total_frames <= 0 || done > total_frames || elapsed_ms < 0 {
        return None;
    }
    let interval = interval.max(1);
    let remaining_frames = i128::from(total_frames - done);
    let eta_seconds =
        i128::from(elapsed_ms).checked_mul(remaining_frames)? / i128::from(done) / 1_000;
    Some(AviQtlExportProgressPlan {
        progress: i32::try_from(i64::from(done) * 100 / i64::from(total_frames)).ok()?,
        current_frame: done,
        total_frames,
        eta_seconds: i32::try_from(eta_seconds.min(i128::from(i32::MAX))).ok()?,
        should_emit: u32::from(done % interval == 0 || done == total_frames),
    })
}

fn codec_backend(codec_name: &str) -> u32 {
    if codec_name.contains("nvenc") {
        CODEC_BACKEND_CUDA
    } else if codec_name.contains("vaapi") {
        CODEC_BACKEND_VAAPI
    } else if codec_name.contains("qsv") {
        CODEC_BACKEND_QSV
    } else if codec_name.contains("d3d11") {
        CODEC_BACKEND_D3D11VA
    } else if codec_name.contains("dxva2") {
        CODEC_BACKEND_DXVA2
    } else if codec_name.contains("videotoolbox") {
        CODEC_BACKEND_VIDEOTOOLBOX
    } else if codec_name.contains("amf") {
        CODEC_BACKEND_AMF
    } else {
        CODEC_BACKEND_SOFTWARE
    }
}

fn codec_fallback(codec_name: &str) -> Option<&'static str> {
    match codec_name {
        "h264_nvenc" | "h264_amf" | "h264_qsv" | "h264_vaapi" => Some("libx264"),
        "hevc_nvenc" | "hevc_amf" | "hevc_qsv" | "hevc_vaapi" => Some("libx265"),
        "av1_nvenc" | "av1_amf" | "av1_vaapi" => Some("libaom-av1"),
        _ => None,
    }
}

fn fixed_gop_mode(codec_name: &str) -> u32 {
    match codec_name {
        "libx264" => FIXED_GOP_X264,
        "libx265" => FIXED_GOP_X265,
        value if value.contains("nvenc") => FIXED_GOP_NVENC,
        _ => FIXED_GOP_NONE,
    }
}

unsafe fn static_bytes(value: &'static str, output_length: *mut usize) -> *const u8 {
    if !slice_is_valid(output_length, 1) {
        return std::ptr::null();
    }
    // SAFETY: The output pointer was validated above and the returned bytes have static lifetime.
    unsafe { output_length.write(value.len()) };
    value.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_export_video_defaults(output: *mut AviQtlExportVideoDefaults) -> u32 {
    if !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The output pointer was validated above.
    unsafe { output.write(video_defaults()) };
    STATUS_OK
}

/// Returns the static default video codec name.
///
/// # Safety
///
/// `output_length` must be writable for one element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_export_default_video_codec(output_length: *mut usize) -> *const u8 {
    // SAFETY: The caller upholds the output pointer contract above.
    unsafe { static_bytes(DEFAULT_VIDEO_CODEC, output_length) }
}

/// Returns the static default audio codec name.
///
/// # Safety
///
/// `output_length` must be writable for one element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_export_default_audio_codec(output_length: *mut usize) -> *const u8 {
    // SAFETY: The caller upholds the output pointer contract above.
    unsafe { static_bytes(DEFAULT_AUDIO_CODEC, output_length) }
}

/// Plans and validates a video export request.
///
/// # Safety
///
/// `request` must be readable and `output` writable for one correctly aligned element. Their
/// memory ranges must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_export_plan_video(
    request: *const AviQtlExportVideoRequest,
    output: *mut AviQtlExportVideoPlan,
) -> u32 {
    if !slice_is_valid(request, 1) || !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(request, 1, output, 1) {
        Some(false) => {}
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        None => return STATUS_INVALID_ARGUMENT,
    }
    // SAFETY: The input and output ranges were validated above and do not overlap.
    unsafe { output.write(plan_video(request.read())) };
    STATUS_OK
}

/// Plans and validates an image-sequence export request.
///
/// # Safety
///
/// `request` must be readable and `output` writable for one correctly aligned element. `format`
/// must be readable for `format_length` bytes. No input range may overlap `output`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_export_plan_image_sequence(
    request: *const AviQtlExportImageSequenceRequest,
    format: *const u8,
    format_length: usize,
    output: *mut AviQtlExportImageSequencePlan,
) -> u32 {
    if !slice_is_valid(request, 1) || !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    for overlap in [
        ranges_overlap(request, 1, output, 1),
        ranges_overlap(format, format_length, output, 1),
    ] {
        match overlap {
            Some(false) => {}
            Some(true) => return STATUS_OVERLAPPING_BUFFERS,
            None => return STATUS_INVALID_ARGUMENT,
        }
    }
    // SAFETY: The caller upholds the readable UTF-8 range contract above.
    let Some(format) = (unsafe { utf8(format, format_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The input and output ranges were validated above and do not overlap.
    unsafe { output.write(plan_image_sequence(request.read(), format)) };
    STATUS_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_export_plan_audio_frame(
    frame_index: i32,
    sample_rate: i32,
    fps_num: i32,
    fps_den: i32,
    output: *mut AviQtlExportAudioFramePlan,
) -> u32 {
    if !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(plan) = plan_audio_frame(frame_index, sample_rate, fps_num, fps_den) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The output pointer was validated above.
    unsafe { output.write(plan) };
    STATUS_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_export_plan_progress(
    done: i32,
    total_frames: i32,
    interval: i32,
    elapsed_ms: i64,
    output: *mut AviQtlExportProgressPlan,
) -> u32 {
    if !slice_is_valid(output, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(plan) = plan_progress(done, total_frames, interval, elapsed_ms) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The output pointer was validated above.
    unsafe { output.write(plan) };
    STATUS_OK
}

/// Classifies the hardware backend implied by one UTF-8 codec name.
///
/// # Safety
///
/// `codec_name` must be readable for `codec_name_length` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_export_codec_backend(
    codec_name: *const u8,
    codec_name_length: usize,
) -> u32 {
    // SAFETY: The caller upholds the readable byte-range contract above.
    unsafe { utf8(codec_name, codec_name_length) }
        .map(|name| export_codec_backend(name) as u32)
        .unwrap_or(CODEC_BACKEND_SOFTWARE)
}

/// Returns a static software fallback codec name, or null when no fallback is defined.
///
/// # Safety
///
/// `codec_name` must be readable for `codec_name_length` bytes and `output_length` must be
/// writable for one element. Their ranges must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_export_codec_fallback(
    codec_name: *const u8,
    codec_name_length: usize,
    output_length: *mut usize,
) -> *const u8 {
    if !slice_is_valid(output_length, 1)
        || !matches!(
            ranges_overlap(codec_name, codec_name_length, output_length, 1),
            Some(false)
        )
    {
        return std::ptr::null();
    }
    // SAFETY: The caller upholds the readable UTF-8 range contract above.
    let fallback = unsafe { utf8(codec_name, codec_name_length) }.and_then(codec_fallback);
    let Some(fallback) = fallback else {
        // SAFETY: The output pointer and non-overlap were validated above.
        unsafe { output_length.write(0) };
        return std::ptr::null();
    };
    // SAFETY: The output pointer contract was validated above.
    unsafe { static_bytes(fallback, output_length) }
}

/// Classifies fixed-GOP controls for one UTF-8 codec name.
///
/// # Safety
///
/// `codec_name` must be readable for `codec_name_length` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_export_fixed_gop_mode(
    codec_name: *const u8,
    codec_name_length: usize,
) -> u32 {
    // SAFETY: The caller upholds the readable byte-range contract above.
    unsafe { utf8(codec_name, codec_name_length) }
        .map(|name| export_fixed_gop_mode(name) as u32)
        .unwrap_or(FIXED_GOP_NONE)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_export_encoder_queue_size(
    width: i32,
    height: i32,
    budget_mb: i32,
) -> usize {
    export_encoder_queue_size(width, height, budget_mb)
}

fn encoder_queue_size(width: i32, height: i32, budget_mb: i32) -> usize {
    let frame_bytes = (width.max(1) as u128) * (height.max(1) as u128) * 4;
    let budget_bytes = (budget_mb.max(16) as u128) * 1024 * 1024;
    let tasks =
        (budget_bytes / frame_bytes).clamp(MIN_ENCODER_QUEUE_TASKS, MAX_ENCODER_QUEUE_TASKS);
    tasks as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplies_the_existing_export_defaults() {
        assert_eq!(
            video_defaults(),
            AviQtlExportVideoDefaults {
                width: 1920,
                height: 1080,
                fps_num: 60_000,
                fps_den: 1_000,
                bitrate: 15_000_000,
                crf: -1,
                gop_size: 0,
                audio_bitrate: 192_000,
                start_frame: 0,
                end_frame: -1,
            }
        );
    }

    #[test]
    fn validates_and_resolves_video_ranges() {
        let request = AviQtlExportVideoRequest {
            width: 1920,
            height: 1080,
            fps_num: 60_000,
            fps_den: 1_001,
            start_frame: 10,
            end_frame: -1,
            timeline_duration: 110,
            output_path_present: 1,
            project_fps: 60_000.0 / 1_001.0,
        };
        assert_eq!(
            plan_video(request),
            AviQtlExportVideoPlan {
                start_frame: 10,
                end_frame: 110,
                total_frames: 100,
                error: CONFIGURATION_OK,
            }
        );
        assert_eq!(
            plan_video(AviQtlExportVideoRequest {
                output_path_present: 0,
                ..request
            })
            .error,
            CONFIGURATION_MISSING_OUTPUT_PATH
        );
        assert_eq!(
            plan_video(AviQtlExportVideoRequest {
                width: 0,
                ..request
            })
            .error,
            CONFIGURATION_INVALID_OUTPUT_SIZE
        );
        assert_eq!(
            plan_video(AviQtlExportVideoRequest {
                fps_den: 0,
                ..request
            })
            .error,
            CONFIGURATION_INVALID_FPS
        );
        assert_eq!(
            plan_video(AviQtlExportVideoRequest {
                start_frame: 110,
                ..request
            })
            .error,
            CONFIGURATION_INVALID_RANGE
        );
        assert_eq!(
            plan_video(AviQtlExportVideoRequest {
                project_fps: 24.0,
                ..request
            })
            .error,
            CONFIGURATION_PROJECT_FPS_MISMATCH
        );
    }

    #[test]
    fn plans_image_format_padding_and_range() {
        let request = AviQtlExportImageSequenceRequest {
            start_frame: 3,
            end_frame: -1,
            timeline_duration: 12_345,
            configured_padding: 4,
            output_path_present: 1,
        };
        assert_eq!(
            plan_image_sequence(request, "JPEG"),
            AviQtlExportImageSequencePlan {
                start_frame: 3,
                end_frame: 12_345,
                total_frames: 12_342,
                pad_digits: 5,
                image_format: IMAGE_FORMAT_JPEG,
                error: CONFIGURATION_OK,
            }
        );
        assert_eq!(
            plan_image_sequence(request, "jpeg").image_format,
            IMAGE_FORMAT_PNG
        );
        assert_eq!(
            plan_image_sequence(
                AviQtlExportImageSequenceRequest {
                    configured_padding: 99,
                    ..request
                },
                "PNG",
            )
            .pad_digits,
            10
        );
    }

    #[test]
    fn audio_plan_uses_the_exact_rational_frame_rate() {
        let first = plan_audio_frame(0, 48_000, 60_000, 1_001).unwrap();
        assert_eq!(first.samples_for_frame, 800);
        assert_eq!(first.cumulative_samples, 800);

        let second = plan_audio_frame(1, 48_000, 60_000, 1_001).unwrap();
        assert_eq!(second.samples_for_frame, 801);
        assert_eq!(second.cumulative_samples, 1_601);

        let one_minute = plan_audio_frame(3_595, 48_000, 60_000, 1_001).unwrap();
        assert_eq!(one_minute.cumulative_samples, 2_879_676);
    }

    #[test]
    fn progress_plan_preserves_interval_percent_and_eta_rules() {
        assert_eq!(
            plan_progress(5, 12, 5, 2_500).unwrap(),
            AviQtlExportProgressPlan {
                progress: 41,
                current_frame: 5,
                total_frames: 12,
                eta_seconds: 3,
                should_emit: 1,
            }
        );
        assert_eq!(plan_progress(6, 12, 5, 3_000).unwrap().should_emit, 0);
        assert_eq!(plan_progress(12, 12, 5, 6_000).unwrap().should_emit, 1);
    }

    #[test]
    fn classifies_codecs_and_queue_budget() {
        assert_eq!(codec_backend("h264_nvenc"), CODEC_BACKEND_CUDA);
        assert_eq!(
            codec_backend("hevc_videotoolbox"),
            CODEC_BACKEND_VIDEOTOOLBOX
        );
        assert_eq!(codec_backend("h264_amf"), CODEC_BACKEND_AMF);
        assert_eq!(codec_backend("libx264"), CODEC_BACKEND_SOFTWARE);
        assert_eq!(codec_fallback("hevc_qsv"), Some("libx265"));
        assert_eq!(codec_fallback("libx264"), None);
        assert_eq!(fixed_gop_mode("libx264"), FIXED_GOP_X264);
        assert_eq!(fixed_gop_mode("h264_nvenc"), FIXED_GOP_NVENC);
        assert_eq!(fixed_gop_mode("h264_vaapi"), FIXED_GOP_NONE);
        assert_eq!(aviqtl_export_encoder_queue_size(1920, 1080, 128), 16);
        assert_eq!(aviqtl_export_encoder_queue_size(7680, 4320, 16), 2);
    }

    #[test]
    fn ffi_rejects_invalid_or_overlapping_ranges_without_writes() {
        let mut untouched = AviQtlExportAudioFramePlan {
            cumulative_samples: 9,
            samples_for_frame: 8,
            reserved: 7,
        };
        assert_eq!(
            aviqtl_export_plan_audio_frame(-1, 48_000, 60_000, 1_001, &mut untouched),
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            untouched,
            AviQtlExportAudioFramePlan {
                cumulative_samples: 9,
                samples_for_frame: 8,
                reserved: 7,
            }
        );

        let mut request = AviQtlExportVideoRequest {
            width: 1920,
            height: 1080,
            fps_num: 60_000,
            fps_den: 1_000,
            start_frame: 0,
            end_frame: 10,
            timeline_duration: 10,
            output_path_present: 1,
            project_fps: 60.0,
        };
        let request_pointer = &mut request as *mut AviQtlExportVideoRequest;
        let overlapping_output = request_pointer.cast::<AviQtlExportVideoPlan>();
        // SAFETY: The pointers are valid for their declared ranges. The function must reject
        // their intentional overlap before writing.
        assert_eq!(
            unsafe { aviqtl_export_plan_video(request_pointer, overlapping_output) },
            STATUS_OVERLAPPING_BUFFERS
        );
        assert_eq!(request.width, 1920);
    }
}
