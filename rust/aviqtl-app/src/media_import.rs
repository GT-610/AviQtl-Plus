use aviqtl_media::{MediaStreamKind, media_duration_seconds};
use std::path::{Path, PathBuf};

const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "aac", "m4a", "flac", "ogg"];
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "webp", "svg"];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "avi", "mkv", "webm", "wmv"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaImportKind {
    Video,
    Audio,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaImportPlan {
    pub path: PathBuf,
    pub kind: MediaImportKind,
    pub duration_frames: i32,
}

pub fn plan_media_import(
    path: &Path,
    scene_fps: f64,
    default_duration_frames: i32,
) -> Result<MediaImportPlan, String> {
    if !path.exists() {
        return Err(format!("file does not exist: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("not a regular file: {}", path.display()));
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    let kind = if extension_matches(extension, VIDEO_EXTENSIONS) {
        MediaImportKind::Video
    } else if extension_matches(extension, AUDIO_EXTENSIONS) {
        MediaImportKind::Audio
    } else if extension_matches(extension, IMAGE_EXTENSIONS) {
        MediaImportKind::Image
    } else {
        return Err(format!("unsupported media format: {extension}"));
    };
    let duration_frames = match kind {
        MediaImportKind::Image => default_duration_frames.max(1),
        MediaImportKind::Video | MediaImportKind::Audio => {
            let stream_kind = if kind == MediaImportKind::Video {
                MediaStreamKind::Video
            } else {
                MediaStreamKind::Audio
            };
            duration_frames_for(
                media_duration_seconds(path, stream_kind).ok().flatten(),
                scene_fps,
                default_duration_frames,
            )
        }
    };
    Ok(MediaImportPlan {
        path: path.to_path_buf(),
        kind,
        duration_frames,
    })
}

fn duration_frames_for(
    probed_seconds: Option<f64>,
    scene_fps: f64,
    default_duration_frames: i32,
) -> i32 {
    probed_seconds
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .filter(|_| scene_fps.is_finite() && scene_fps > 0.0)
        .map(|seconds| (seconds * scene_fps).ceil().clamp(1.0, f64::from(i32::MAX)) as i32)
        .unwrap_or_else(|| default_duration_frames.max(1))
}

fn extension_matches(extension: &str, supported: &[&str]) -> bool {
    supported
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_file(extension: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aviqtl-media-import-{}-{nonce}.{extension}",
            std::process::id()
        ));
        fs::write(&path, []).expect("temporary media placeholder is writable");
        path
    }

    #[test]
    fn import_classification_matches_qt_extensions_and_fallback_duration() {
        let video = temporary_file("MOV");
        let audio = temporary_file("flac");
        let image = temporary_file("webp");

        let video_plan = plan_media_import(&video, 60.0, 100).expect("video is recognized");
        let audio_plan = plan_media_import(&audio, 60.0, 100).expect("audio is recognized");
        let image_plan = plan_media_import(&image, 60.0, 100).expect("image is recognized");
        assert_eq!(video_plan.kind, MediaImportKind::Video);
        assert_eq!(audio_plan.kind, MediaImportKind::Audio);
        assert_eq!(image_plan.kind, MediaImportKind::Image);
        assert_eq!(video_plan.duration_frames, 100);
        assert_eq!(audio_plan.duration_frames, 100);
        assert_eq!(image_plan.duration_frames, 100);

        fs::remove_file(video).expect("temporary video placeholder is removable");
        fs::remove_file(audio).expect("temporary audio placeholder is removable");
        fs::remove_file(image).expect("temporary image placeholder is removable");
    }

    #[test]
    fn unsupported_missing_and_directory_inputs_are_rejected() {
        let unsupported = temporary_file("txt");
        let missing = unsupported.with_extension("png.missing");
        assert!(plan_media_import(&unsupported, 60.0, 100).is_err());
        assert!(plan_media_import(&missing, 60.0, 100).is_err());
        assert!(plan_media_import(std::env::temp_dir().as_path(), 60.0, 100).is_err());
        fs::remove_file(unsupported).expect("temporary placeholder is removable");
    }

    #[test]
    fn probed_duration_uses_qt_ceil_conversion_and_safe_fallbacks() {
        assert_eq!(duration_frames_for(Some(1.01), 30.0, 100), 31);
        assert_eq!(duration_frames_for(Some(0.01), 60.0, 100), 1);
        assert_eq!(duration_frames_for(None, 60.0, 100), 100);
        assert_eq!(duration_frames_for(Some(f64::NAN), 60.0, 100), 100);
        assert_eq!(duration_frames_for(Some(1.0), 0.0, 100), 100);
    }
}
