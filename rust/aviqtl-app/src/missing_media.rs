use aviqtl_media::{MediaStreamKind, media_duration_seconds};
use aviqtl_rust_core::api::{ClipDocument, ProjectDocument};
use std::path::{Path, PathBuf};

const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "aac", "m4a", "flac", "ogg"];
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "webp", "svg"];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "avi", "mkv", "webm", "wmv"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingMediaEntry {
    pub clip_id: i32,
    pub scene_id: i32,
    pub layer: i32,
    pub clip_type: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaRelinkPlan {
    pub clip_id: i32,
    pub effect_index: usize,
    pub parameter: &'static str,
    pub path: String,
    pub media_duration_seconds: Option<f64>,
}

pub fn find_missing_media(
    document: &ProjectDocument,
    project_path: Option<&Path>,
) -> Vec<MissingMediaEntry> {
    document
        .clips
        .iter()
        .filter_map(|clip| {
            let (_, parameter) = media_effect_target(clip)?;
            let path = clip
                .effects
                .iter()
                .find(|effect| effect.id == clip.clip_type)?
                .params
                .get(parameter)?
                .as_str()?;
            if path.is_empty() || resolve_path(project_path, path).exists() {
                return None;
            }
            Some(MissingMediaEntry {
                clip_id: clip.id,
                scene_id: clip.scene_id,
                layer: clip.layer,
                clip_type: clip.clip_type.clone(),
                path: path.to_owned(),
                name: Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

pub fn plan_media_relink(
    document: &ProjectDocument,
    clip_id: i32,
    path: &Path,
) -> Result<MediaRelinkPlan, String> {
    let clip = document
        .clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("clip #{clip_id} does not exist"))?;
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
    let supported = match clip.clip_type.as_str() {
        "audio" => extension_matches(extension, AUDIO_EXTENSIONS),
        "image" => extension_matches(extension, IMAGE_EXTENSIONS),
        "video" => extension_matches(extension, VIDEO_EXTENSIONS),
        other => return Err(format!("media type cannot be relinked: {other}")),
    };
    if !supported {
        return Err(format!("unsupported media format: {extension}"));
    }
    let (effect_index, parameter) = media_effect_target(clip)
        .ok_or_else(|| format!("media effect is missing from clip #{clip_id}"))?;
    let duration = (clip.clip_type == "audio")
        .then(|| {
            media_duration_seconds(path, MediaStreamKind::Audio)
                .ok()
                .flatten()
        })
        .flatten();
    Ok(MediaRelinkPlan {
        clip_id,
        effect_index,
        parameter,
        path: path.to_string_lossy().into_owned(),
        media_duration_seconds: duration,
    })
}

fn media_effect_target(clip: &ClipDocument) -> Option<(usize, &'static str)> {
    let parameter = match clip.clip_type.as_str() {
        "audio" => "source",
        "image" | "video" => "path",
        _ => return None,
    };
    clip.effects
        .iter()
        .position(|effect| effect.id == clip.clip_type)
        .map(|index| (index, parameter))
}

fn resolve_path(project_path: Option<&Path>, source: &str) -> PathBuf {
    let path = PathBuf::from(source);
    if path.is_absolute() {
        path
    } else {
        project_path
            .and_then(Path::parent)
            .map_or(path.clone(), |directory| directory.join(path))
    }
}

fn extension_matches(extension: &str, supported: &[&str]) -> bool {
    supported
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::TimelineState;
    use serde_json::json;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn document_with_media(path: &str, clip_type: &str) -> ProjectDocument {
        let parameter = if clip_type == "audio" {
            "source"
        } else {
            "path"
        };
        let mut params = serde_json::Map::new();
        params.insert(parameter.to_owned(), json!(path));
        let document = json!({
            "version": 3,
            "settings": {"width": 1920, "height": 1080, "fps": 60.0, "sampleRate": 48000},
            "scenes": [{"id": 1, "name": "Root", "duration": 600}],
            "clips": [{
                "id": 7,
                "sceneId": 1,
                "type": clip_type,
                "start": 0,
                "duration": 60,
                "layer": 2,
                "effects": [{"id": clip_type, "params": params}]
            }]
        });
        TimelineState::from_json(&serde_json::to_vec(&document).expect("fixture serializes"))
            .expect("fixture parses")
            .snapshot()
    }

    fn temporary_path(extension: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "aviqtl-missing-media-{}-{nonce}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn missing_media_scan_resolves_paths_relative_to_the_project() {
        let project_path = Path::new("/tmp/project/example.aviqtl");
        let document = document_with_media("media/missing.png", "image");
        let missing = find_missing_media(&document, Some(project_path));
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].clip_id, 7);
        assert_eq!(missing[0].name, "missing.png");
    }

    #[test]
    fn relink_validates_type_extension_and_effect_target() {
        let path = temporary_path("PNG");
        fs::write(&path, b"fixture").expect("fixture writes");
        let document = document_with_media("missing.png", "image");
        let plan = plan_media_relink(&document, 7, &path).expect("image relink plans");
        assert_eq!(plan.effect_index, 0);
        assert_eq!(plan.parameter, "path");
        assert_eq!(plan.path, path.to_string_lossy());

        let audio = document_with_media("missing.wav", "audio");
        assert!(plan_media_relink(&audio, 7, &path).is_err());
        fs::remove_file(path).expect("fixture removes");
    }
}
