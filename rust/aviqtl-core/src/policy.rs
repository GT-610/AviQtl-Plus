use crate::abi::{slice_is_valid, utf8};

const DEFAULT_SPEED: f64 = 100.0;

const AUDIO_DURATION_PARAMETERS: [&str; 5] =
    ["source", "startTime", "speed", "playMode", "linkedVideo"];

const AUDIO_WAVEFORM_PARAMETERS: [&str; 14] = [
    "source",
    "linkedVideo",
    "playMode",
    "startTime",
    "speed",
    "directTime",
    "volume",
    "masterVolume",
    "pan",
    "fadeIn",
    "fadeOut",
    "mute",
    "solo",
    "limiter",
];

pub(crate) const PERMISSION_NAMES: [&str; 13] = [
    "transport.control",
    "clip.read",
    "clip.modify",
    "effect.modify",
    "project.read",
    "project.save",
    "project.load",
    "scene.manage",
    "settings.read",
    "settings.write",
    "clipboard.access",
    "history.control",
    "log.output",
];

const API_PERMISSIONS: [(&str, &str); 33] = [
    ("transport_play", "transport.control"),
    ("transport_pause", "transport.control"),
    ("transport_toggle", "transport.control"),
    ("transport_seek", "transport.control"),
    ("transport_get_frame", "transport.control"),
    ("transport_is_playing", "transport.control"),
    ("clip_list", "clip.read"),
    ("clip_select", "clip.read"),
    ("clip_create", "clip.modify"),
    ("clip_delete", "clip.modify"),
    ("clip_update", "clip.modify"),
    ("clip_split", "clip.modify"),
    ("clip_copy", "clipboard.access"),
    ("clip_cut", "clipboard.access"),
    ("clip_paste", "clipboard.access"),
    ("effect_add", "effect.modify"),
    ("effect_remove", "effect.modify"),
    ("effect_set_param", "effect.modify"),
    ("project_width", "project.read"),
    ("project_height", "project.read"),
    ("project_fps", "project.read"),
    ("project_save", "project.save"),
    ("project_load", "project.load"),
    ("scene_create", "scene.manage"),
    ("scene_remove", "scene.manage"),
    ("scene_switch", "scene.manage"),
    ("settings_set", "settings.write"),
    ("settings_get", "settings.read"),
    ("undo", "history.control"),
    ("redo", "history.control"),
    ("command_begin_group", "history.control"),
    ("command_end_group", "history.control"),
    ("log", "log.output"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub(crate) enum PlaybackMode {
    Normal = 0,
    Direct = 1,
}

pub(crate) fn playback_mode(value: &str) -> Option<PlaybackMode> {
    match value {
        "normal"
        | "開始フレーム＋再生速度"
        | "開始時間＋再生速度"
        | "Start Frame + Playback Speed"
        | "Start Time + Playback Speed"
        | "起始帧＋播放速度"
        | "开始时间＋播放速度" => Some(PlaybackMode::Normal),
        "direct"
        | "フレーム直接指定"
        | "時間直接指定"
        | "Direct Frame"
        | "Direct Time"
        | "直接指定帧"
        | "直接指定时间" => Some(PlaybackMode::Direct),
        _ => None,
    }
}

pub(crate) fn canonical_playback_mode(value: &str) -> Option<&'static str> {
    match playback_mode(value)? {
        PlaybackMode::Normal => Some("normal"),
        PlaybackMode::Direct => Some("direct"),
    }
}

pub(crate) fn is_video_file(value: &str) -> bool {
    let lower = value.to_lowercase();
    [".mp4", ".mov", ".avi", ".mkv", ".webm", ".wmv"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

pub(crate) fn audio_parameter_affects_duration(value: &str) -> bool {
    AUDIO_DURATION_PARAMETERS.contains(&value)
}

fn audio_parameter_affects_waveform(value: &str) -> bool {
    AUDIO_WAVEFORM_PARAMETERS.contains(&value)
}

fn resolve_audio_time(
    relative_time: f64,
    direct_mode: bool,
    direct_time: f64,
    start_time: f64,
    speed: f64,
) -> f64 {
    if direct_mode {
        direct_time
    } else {
        relative_time * (speed / DEFAULT_SPEED) + start_time
    }
}

fn resolve_video_time(
    relative_frame: i32,
    source_fps: f64,
    direct_mode: bool,
    direct_frame: f64,
    start_frame: f64,
    speed: f64,
) -> f64 {
    if !source_fps.is_finite() || source_fps <= 0.0 {
        return 0.0;
    }
    if direct_mode {
        return direct_frame / source_fps;
    }
    let start_seconds = start_frame / source_fps;
    let relative_time = f64::from(relative_frame) / source_fps;
    start_seconds + relative_time * (speed / DEFAULT_SPEED)
}

fn max_video_duration_frames(
    total_frame_count: i32,
    source_fps: f64,
    speed: f64,
    start_frame: f64,
    project_fps: i32,
) -> i32 {
    if total_frame_count <= 0
        || !speed.is_finite()
        || speed <= 0.0
        || !source_fps.is_finite()
        || source_fps <= 0.0
        || !start_frame.is_finite()
        || project_fps <= 0
    {
        return 0;
    }
    let start_seconds = start_frame / source_fps;
    let remaining_seconds = f64::from(total_frame_count) / source_fps - start_seconds;
    if remaining_seconds <= 0.0 {
        return 0;
    }
    let frames = remaining_seconds / (speed / DEFAULT_SPEED) * f64::from(project_fps);
    if !frames.is_finite() || frames <= 0.0 {
        0
    } else {
        frames.min(f64::from(i32::MAX)) as i32
    }
}

fn clamp_video_duration_frames(
    requested_duration: i32,
    total_frame_count: i32,
    source_fps: f64,
    direct_mode: bool,
    start_frame: f64,
    speed: f64,
    project_fps: i32,
) -> i32 {
    if total_frame_count <= 0 || !source_fps.is_finite() || source_fps <= 0.0 || project_fps <= 0 {
        return requested_duration;
    }
    let maximum = if direct_mode {
        f64::from(total_frame_count) / source_fps * f64::from(project_fps)
    } else if speed.is_finite() && speed > 0.0 && start_frame.is_finite() {
        let remaining = f64::from(total_frame_count) / source_fps - start_frame / source_fps;
        if remaining <= 0.0 {
            return requested_duration;
        }
        remaining / (speed / DEFAULT_SPEED) * f64::from(project_fps)
    } else {
        return requested_duration;
    };
    let maximum = maximum.clamp(0.0, f64::from(i32::MAX)) as i32;
    if maximum > 0 && requested_duration > maximum {
        maximum
    } else {
        requested_duration
    }
}

fn clamp_audio_duration_frames(
    requested_duration: i32,
    total_seconds: f64,
    direct_mode: bool,
    start_time: f64,
    speed: f64,
    project_fps: i32,
) -> i32 {
    if !total_seconds.is_finite() || total_seconds <= 0.0 || project_fps <= 0 {
        return requested_duration;
    }
    let maximum = if direct_mode {
        total_seconds * f64::from(project_fps)
    } else if speed.is_finite() && speed > 0.0 && start_time.is_finite() {
        let remaining = total_seconds - start_time;
        if remaining <= 0.0 {
            return requested_duration;
        }
        remaining / (speed / DEFAULT_SPEED) * f64::from(project_fps)
    } else {
        return requested_duration;
    };
    let maximum = maximum.clamp(0.0, f64::from(i32::MAX)) as i32;
    if maximum > 0 && requested_duration > maximum {
        maximum
    } else {
        requested_duration
    }
}

pub(crate) fn audio_duration_frames(
    total_seconds: f64,
    direct_mode: bool,
    start_time: f64,
    speed: f64,
    project_fps: f64,
) -> i32 {
    if !total_seconds.is_finite()
        || total_seconds <= 0.0
        || !project_fps.is_finite()
        || project_fps <= 0.0
    {
        return 0;
    }
    let duration = if direct_mode {
        total_seconds
    } else {
        if !start_time.is_finite() || start_time < 0.0 || !speed.is_finite() || speed <= 0.0 {
            return 0;
        }
        let remaining = total_seconds - start_time;
        if remaining <= 0.0 {
            return 0;
        }
        remaining / (speed / DEFAULT_SPEED)
    };
    let frames = (duration * project_fps).ceil();
    if !frames.is_finite() || frames <= 0.0 {
        0
    } else {
        frames.clamp(1.0, f64::from(i32::MAX)) as i32
    }
}

pub(crate) fn permission_from_name(value: &str) -> i32 {
    PERMISSION_NAMES
        .iter()
        .position(|name| *name == value)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or(-1)
}

fn permission_for_api(value: &str) -> i32 {
    API_PERMISSIONS
        .iter()
        .find_map(|(name, permission)| (*name == value).then(|| permission_from_name(permission)))
        .unwrap_or(-1)
}

pub(crate) fn valid_package_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

fn package_type(value: &str) -> i32 {
    match value {
        "mod" => 0,
        "effect" => 1,
        "object" => 2,
        _ => -1,
    }
}

fn safe_archive_path(value: &str) -> bool {
    let has_drive_prefix = value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    if value.is_empty()
        || value.contains('\0')
        || value.contains('\\')
        || value.starts_with('/')
        || value.starts_with("//")
        || has_drive_prefix
    {
        return false;
    }
    let mut depth = 0_usize;
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next_depth;
            }
            _ => depth += 1,
        }
    }
    true
}

pub(crate) fn valid_recovery_id(value: &str) -> bool {
    if value.len() != 36
        || !value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
    {
        return false;
    }
    value.bytes().any(|byte| byte != b'0' && byte != b'-')
}

pub(crate) fn valid_recovery_snapshot_name(id: &str, file_name: &str) -> bool {
    if !valid_recovery_id(id)
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains('\0')
    {
        return false;
    }
    if file_name == format!("{id}.aviqtl") {
        return true;
    }
    let Some(generation) = file_name
        .strip_prefix(&format!("{id}-"))
        .and_then(|name| name.strip_suffix(".aviqtl"))
    else {
        return false;
    };
    valid_recovery_id(generation)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_media_playback_mode(value: *const u8, value_length: usize) -> i32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    unsafe { utf8(value, value_length) }
        .and_then(playback_mode)
        .map_or(-1, |mode| mode as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_media_is_video_file(value: *const u8, value_length: usize) -> u32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    u32::from(unsafe { utf8(value, value_length) }.is_some_and(is_video_file))
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_audio_parameter_affects_duration(
    value: *const u8,
    value_length: usize,
) -> u32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    u32::from(unsafe { utf8(value, value_length) }.is_some_and(audio_parameter_affects_duration))
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_audio_parameter_affects_waveform(
    value: *const u8,
    value_length: usize,
) -> u32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    u32::from(unsafe { utf8(value, value_length) }.is_some_and(audio_parameter_affects_waveform))
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_media_resolve_audio_time(
    relative_time: f64,
    direct_mode: u32,
    direct_time: f64,
    start_time: f64,
    speed: f64,
) -> f64 {
    resolve_audio_time(
        relative_time,
        direct_mode != 0,
        direct_time,
        start_time,
        speed,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_media_resolve_video_time(
    relative_frame: i32,
    source_fps: f64,
    direct_mode: u32,
    direct_frame: f64,
    start_frame: f64,
    speed: f64,
) -> f64 {
    resolve_video_time(
        relative_frame,
        source_fps,
        direct_mode != 0,
        direct_frame,
        start_frame,
        speed,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_media_max_video_duration_frames(
    total_frame_count: i32,
    source_fps: f64,
    speed: f64,
    start_frame: f64,
    project_fps: i32,
) -> i32 {
    max_video_duration_frames(
        total_frame_count,
        source_fps,
        speed,
        start_frame,
        project_fps,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_media_clamp_video_duration_frames(
    requested_duration: i32,
    total_frame_count: i32,
    source_fps: f64,
    direct_mode: u32,
    start_frame: f64,
    speed: f64,
    project_fps: i32,
) -> i32 {
    clamp_video_duration_frames(
        requested_duration,
        total_frame_count,
        source_fps,
        direct_mode != 0,
        start_frame,
        speed,
        project_fps,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_media_clamp_audio_duration_frames(
    requested_duration: i32,
    total_seconds: f64,
    direct_mode: u32,
    start_time: f64,
    speed: f64,
    project_fps: i32,
) -> i32 {
    clamp_audio_duration_frames(
        requested_duration,
        total_seconds,
        direct_mode != 0,
        start_time,
        speed,
        project_fps,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_permission_from_name(value: *const u8, value_length: usize) -> i32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    unsafe { utf8(value, value_length) }.map_or(-1, permission_from_name)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_permission_for_api(value: *const u8, value_length: usize) -> i32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    unsafe { utf8(value, value_length) }.map_or(-1, permission_for_api)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_permission_count() -> i32 {
    PERMISSION_NAMES.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_name(
    permission: i32,
    output_length: *mut usize,
) -> *const u8 {
    if !slice_is_valid(output_length, 1) {
        return std::ptr::null();
    }
    let Some(name) = usize::try_from(permission)
        .ok()
        .and_then(|permission| PERMISSION_NAMES.get(permission))
    else {
        // SAFETY: The output was validated above.
        unsafe { output_length.write(0) };
        return std::ptr::null();
    };
    // SAFETY: The output was validated above. The returned string has static lifetime.
    unsafe { output_length.write(name.len()) };
    name.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_package_id_is_valid(value: *const u8, value_length: usize) -> u32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    u32::from(unsafe { utf8(value, value_length) }.is_some_and(valid_package_id))
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_package_type(value: *const u8, value_length: usize) -> i32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    unsafe { utf8(value, value_length) }.map_or(-1, package_type)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_package_archive_path_is_safe(
    value: *const u8,
    value_length: usize,
) -> u32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    u32::from(unsafe { utf8(value, value_length) }.is_some_and(safe_archive_path))
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_recovery_id_is_valid(value: *const u8, value_length: usize) -> u32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    u32::from(unsafe { utf8(value, value_length) }.is_some_and(valid_recovery_id))
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_recovery_snapshot_name_is_valid(
    id: *const u8,
    id_length: usize,
    file_name: *const u8,
    file_name_length: usize,
) -> u32 {
    // SAFETY: Both helpers validate their pointer/length pairs before borrowing them.
    u32::from(
        unsafe { utf8(id, id_length) }
            .zip(unsafe { utf8(file_name, file_name_length) })
            .is_some_and(|(id, file_name)| valid_recovery_snapshot_name(id, file_name)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_policy_matches_timeline_semantics() {
        assert_eq!(playback_mode("normal"), Some(PlaybackMode::Normal));
        assert_eq!(playback_mode("direct"), Some(PlaybackMode::Direct));
        assert_eq!(
            playback_mode("開始フレーム＋再生速度"),
            Some(PlaybackMode::Normal)
        );
        assert_eq!(
            playback_mode("開始時間＋再生速度"),
            Some(PlaybackMode::Normal)
        );
        assert_eq!(
            playback_mode("フレーム直接指定"),
            Some(PlaybackMode::Direct)
        );
        assert_eq!(playback_mode("時間直接指定"), Some(PlaybackMode::Direct));
        assert_eq!(
            playback_mode("Start Frame + Playback Speed"),
            Some(PlaybackMode::Normal)
        );
        assert_eq!(
            playback_mode("Start Time + Playback Speed"),
            Some(PlaybackMode::Normal)
        );
        assert_eq!(playback_mode("Direct Frame"), Some(PlaybackMode::Direct));
        assert_eq!(playback_mode("Direct Time"), Some(PlaybackMode::Direct));
        assert_eq!(
            playback_mode("起始帧＋播放速度"),
            Some(PlaybackMode::Normal)
        );
        assert_eq!(
            playback_mode("开始时间＋播放速度"),
            Some(PlaybackMode::Normal)
        );
        assert_eq!(playback_mode("直接指定帧"), Some(PlaybackMode::Direct));
        assert_eq!(playback_mode("直接指定时间"), Some(PlaybackMode::Direct));
        assert_eq!(playback_mode("モード: 直接"), None);
        assert_eq!(canonical_playback_mode("時間直接指定"), Some("direct"));
        assert!(is_video_file("CLIP.MOV"));
        assert!(!is_video_file("clip.mp4.bak"));
        assert!(audio_parameter_affects_duration("startTime"));
        assert!(!audio_parameter_affects_duration("volume"));
        assert!(audio_parameter_affects_waveform("volume"));
        assert!(!audio_parameter_affects_waveform("unrelated"));
        assert_eq!(resolve_audio_time(2.0, false, 0.0, 1.0, 200.0), 5.0);
        assert_eq!(resolve_video_time(30, 30.0, false, 0.0, 60.0, 100.0), 3.0);
        assert_eq!(max_video_duration_frames(100, 30.0, 100.0, 30.0, 30), 70);
        assert_eq!(
            clamp_video_duration_frames(200, 100, 25.0, true, 0.0, 100.0, 50),
            200
        );
        assert_eq!(
            clamp_video_duration_frames(200, 100, 25.0, false, 25.0, 100.0, 50),
            150
        );
        assert_eq!(
            clamp_audio_duration_frames(500, 10.0, true, 0.0, 100.0, 30),
            300
        );
        assert_eq!(
            clamp_audio_duration_frames(500, 10.0, false, 2.0, 200.0, 30),
            120
        );
        assert_eq!(audio_duration_frames(10.01, true, 0.0, 100.0, 30.0), 301);
        assert_eq!(audio_duration_frames(10.0, false, 2.0, 200.0, 30.0), 120);
        assert_eq!(audio_duration_frames(10.0, false, f64::NAN, 100.0, 30.0), 0);
    }

    #[test]
    fn permission_tables_are_complete_and_reversible() {
        for (index, name) in PERMISSION_NAMES.iter().enumerate() {
            assert_eq!(permission_from_name(name), index as i32);
        }
        for (api, permission) in API_PERMISSIONS {
            assert_eq!(permission_for_api(api), permission_from_name(permission));
        }
        assert_eq!(permission_for_api("clip_copy"), 10);
        assert_eq!(permission_for_api("unknown"), -1);
    }

    #[test]
    fn package_policy_rejects_escape_paths_and_invalid_ids() {
        assert!(valid_package_id("org.aviqtl.example-1"));
        assert!(!valid_package_id("../example"));
        assert_eq!(package_type("effect"), 1);
        assert_eq!(package_type("unsupported"), -1);
        assert!(safe_archive_path("wrapper/../effect/main.qml"));
        assert!(!safe_archive_path("../effect/main.qml"));
        assert!(!safe_archive_path("effect\\main.qml"));
        assert!(!safe_archive_path("/absolute/main.qml"));
        assert!(!safe_archive_path("C:/absolute/main.qml"));
    }

    #[test]
    fn recovery_policy_accepts_only_canonical_ids_and_owned_snapshots() {
        let id = "01234567-89ab-cdef-0123-456789abcdef";
        let generation = "fedcba98-7654-3210-fedc-ba9876543210";
        assert!(valid_recovery_id(id));
        assert!(!valid_recovery_id("00000000-0000-0000-0000-000000000000"));
        assert!(!valid_recovery_id("01234567-89AB-cdef-0123-456789abcdef"));
        assert!(valid_recovery_snapshot_name(id, &format!("{id}.aviqtl")));
        assert!(valid_recovery_snapshot_name(
            id,
            &format!("{id}-{generation}.aviqtl")
        ));
        assert!(!valid_recovery_snapshot_name(
            id,
            &format!("../{id}.aviqtl")
        ));
        assert!(!valid_recovery_snapshot_name(
            id,
            &format!("{generation}.aviqtl")
        ));
    }
}
