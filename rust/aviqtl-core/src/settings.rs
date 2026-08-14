use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use serde_json::{Map, Value, json};

// Keep these defaults aligned with core/include/constants.hpp.
const DEFAULT_WIDTH: i32 = 1920;
const DEFAULT_HEIGHT: i32 = 1080;
const DEFAULT_FPS: f64 = 60.0;
const DEFAULT_SAMPLE_RATE: i32 = 48_000;
const DEFAULT_CLIP_DURATION: i32 = 100;
const AUDIO_MAX_BLOCK_SIZE: i32 = 4096;

const PLATFORM_PATH_KEYS: [&str; 11] = [
    "pluginPathsLADSPA",
    "pluginPathsDSSI",
    "pluginPathsLV2",
    "pluginPathsVST2",
    "pluginPathsVST3",
    "pluginPathsCLAP",
    "pluginPathsSF2",
    "pluginPathsSFZ",
    "pluginPathsJSFX",
    "pluginPathsEffects",
    "pluginPathsObjects",
];

fn shortcuts() -> Value {
    json!({
        "project.new": "Ctrl+N",
        "project.save": "Ctrl+S",
        "project.open": "Ctrl+O",
        "project.saveAs": "Ctrl+Shift+S",
        "project.export": "Ctrl+E",
        "app.quit": "Ctrl+Q",
        "app.settings": "Ctrl+P",
        "edit.undo": "Ctrl+Z",
        "edit.redo": "Ctrl+Shift+Z",
        "edit.cut": "Ctrl+X",
        "edit.copy": "Ctrl+C",
        "edit.paste": "Ctrl+V",
        "edit.delete": "Delete",
        "edit.duplicate": "Ctrl+D",
        "transport.playPause": "Space",
        "transport.nextFrame": "Right",
        "transport.prevFrame": "Left",
        "transport.jumpStart": "Home",
        "transport.jumpEnd": "End",
        "view.zoomIn": "Ctrl++",
        "view.zoomOut": "Ctrl+-",
        "view.timeline": "F3",
        "view.objectSettings": "F4",
        "project.settings": "Alt+Enter",
        "timeline.split": "S",
        "timeline.moveUp": "Alt+Up",
        "timeline.moveDown": "Alt+Down",
        "timeline.nudgeLeft": "Alt+Left",
        "timeline.nudgeRight": "Alt+Right",
        "timeline.addScene": "Ctrl+T",
        "timeline.sceneSettings": "Alt+S",
        "timeline.removeScene": "Ctrl+Shift+Delete",
        "timeline.layerLock": "Ctrl+L",
        "timeline.layerHide": "Ctrl+H"
    })
}

fn default_settings(platform_defaults: &Map<String, Value>) -> Map<String, Value> {
    let mut settings = json!({
        "pluginEnableLADSPA": true,
        "pluginEnableDSSI": true,
        "pluginEnableLV2": true,
        "pluginEnableVST2": true,
        "pluginEnableVST3": true,
        "pluginEnableCLAP": true,
        "pluginEnableSF2": true,
        "pluginEnableSFZ": true,
        "pluginEnableJSFX": true,
        "pluginEnableEffects": true,
        "pluginEnableObjects": true,
        "packageRepositories": [{
            "url": "https://raw.githubusercontent.com/GT-610/AviQtl-Plus/main/repos/repo.json",
            "name": "AviQtl Official",
            "enabled": true,
            "priority": 10
        }],
        "maxImageSize": 8192,
        "cacheSize": 512,
        "undoCount": 32,
        "theme": "Dark",
        "showConfirmOnClose": true,
        "enableAutoBackup": true,
        "backupInterval": 5,
        "defaultProjectWidth": DEFAULT_WIDTH,
        "defaultProjectHeight": DEFAULT_HEIGHT,
        "defaultProjectFps": DEFAULT_FPS,
        "defaultProjectFrames": 3600,
        "defaultProjectSampleRate": DEFAULT_SAMPLE_RATE,
        "defaultClipDuration": DEFAULT_CLIP_DURATION,
        "enableSnap": true,
        "enableTimelineSkimming": true,
        "timelineTrackHeight": 30,
        "timelineHeaderHeight": 28,
        "timelineRulerHeight": 32,
        "timelineMaxLayers": 128,
        "timelineLayerHeaderWidth": 60,
        "timelineClipResizeHandleWidth": 10,
        "splashSize": 512,
        "exportImageQuality": 95,
        "exportSequencePadding": 6,
        "minClipDurationFrames": 5,
        "magneticSnapRange": 10,
        "timelineZoomMin": 10,
        "timelineZoomMax": 400,
        "timelineZoomStep": 10,
        "videoDecoderIndexReserve": 108000,
        "videoDecoderMinCacheMB": 64,
        "hwFramePoolSize": 32,
        "bakeStrategy": "OnDemand",
        "onDemandPrefetchFrames": 30,
        "previewRenderScale": 1.0,
        "previewMsaaSamples": 0,
        "exportDefaultCodec": "libx264",
        "exportDefaultBitrateMbps": 15,
        "exportDefaultCrf": 20,
        "exportDefaultAudioCodec": "aac",
        "exportDefaultAudioBitrateKbps": 192,
        "exportFrameGrabTimeoutMs": 2000,
        "exportProgressInterval": 5,
        "exportEncoderQueueMB": 128,
        "audioPluginMaxBlockSize": AUDIO_MAX_BLOCK_SIZE,
        "sceneWidthMax": 8000,
        "sceneHeightMax": 8000,
        "sceneFramesMin": 100,
        "sceneFramesMax": 24000,
        "sceneFramesStep": 100,
        "recentProjectMaxCount": 10,
        "luaHookIntervalMs": 16,
        "shortcuts": shortcuts()
    })
    .as_object()
    .cloned()
    .expect("settings defaults must be an object");

    for key in PLATFORM_PATH_KEYS {
        settings.insert(
            key.to_owned(),
            platform_defaults
                .get(key)
                .filter(|value| value.is_array())
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        );
    }
    settings
}

fn merge_settings(
    base: &Map<String, Value>,
    loaded: &Map<String, Value>,
) -> (Map<String, Value>, bool) {
    let mut settings = base.clone();
    for (key, value) in loaded {
        if key == "shortcuts" && value.is_object() {
            let mut merged = settings
                .get("shortcuts")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            merged.extend(value.as_object().cloned().unwrap_or_default());
            settings.insert(key.clone(), Value::Object(merged));
        } else {
            settings.insert(key.clone(), value.clone());
        }
    }

    let migrated = loaded.contains_key("packageRepositoryUrls");
    if let Some(legacy) = loaded.get("packageRepositoryUrls") {
        if !loaded.contains_key("packageRepositories") {
            let repositories = legacy
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|url| {
                    json!({
                        "url": url,
                        "name": url,
                        "enabled": true,
                        "priority": 10
                    })
                })
                .collect();
            settings.insert("packageRepositories".to_owned(), Value::Array(repositories));
        }
        settings.remove("packageRepositoryUrls");
    }
    (settings, migrated)
}

fn persistent_settings(settings: &Map<String, Value>) -> Map<String, Value> {
    settings
        .iter()
        .filter(|(key, _)| !key.starts_with('_'))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn parse_object(input: &[u8]) -> Option<Map<String, Value>> {
    serde_json::from_slice::<Value>(input)
        .ok()?
        .as_object()
        .cloned()
}

unsafe fn input_bytes<'a>(input: *const u8, input_length: usize) -> &'a [u8] {
    if input_length == 0 {
        &[]
    } else {
        // SAFETY: Callers validate the full input range before invoking this helper.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    }
}

unsafe fn write_json(
    json: Vec<u8>,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: The caller validates and de-overlaps the output-length range.
    unsafe { output_length.write(json.len()) };
    if output_capacity < json.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !json.is_empty() {
        // SAFETY: The caller validates the output range and capacity.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..json.len()].copy_from_slice(&json);
    }
    STATUS_OK
}

fn single_ranges_valid(
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> Result<(), u32> {
    if !slice_is_valid(input, input_length)
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
    {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    let overlaps = [
        ranges_overlap(input, input_length, output, output_capacity),
        ranges_overlap(input, input_length, output_length, 1),
        ranges_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return Err(STATUS_OVERLAPPING_BUFFERS);
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_defaults_json(
    platform_defaults: *const u8,
    platform_defaults_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if let Err(status) = single_ranges_valid(
        platform_defaults,
        platform_defaults_length,
        output,
        output_capacity,
        output_length,
    ) {
        return status;
    }
    // SAFETY: The input range was validated above.
    let input = unsafe { input_bytes(platform_defaults, platform_defaults_length) };
    let Some(platform_defaults) = parse_object(input) else {
        return STATUS_INVALID_JSON;
    };
    let Ok(json) = serde_json::to_vec(&default_settings(&platform_defaults)) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: Output ranges were validated and checked for overlap above.
    unsafe { write_json(json, output, output_capacity, output_length) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_merge_json(
    base: *const u8,
    base_length: usize,
    loaded: *const u8,
    loaded_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
    migrated: *mut u32,
) -> u32 {
    if !slice_is_valid(base, base_length)
        || !slice_is_valid(loaded, loaded_length)
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
        || !slice_is_valid(migrated, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        ranges_overlap(base, base_length, output, output_capacity),
        ranges_overlap(base, base_length, output_length, 1),
        ranges_overlap(base, base_length, migrated, 1),
        ranges_overlap(loaded, loaded_length, output, output_capacity),
        ranges_overlap(loaded, loaded_length, output_length, 1),
        ranges_overlap(loaded, loaded_length, migrated, 1),
        ranges_overlap(output, output_capacity, output_length, 1),
        ranges_overlap(output, output_capacity, migrated, 1),
        ranges_overlap(output_length, 1, migrated, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }

    // SAFETY: Both input ranges were validated above.
    let Some(base) = parse_object(unsafe { input_bytes(base, base_length) }) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: Both input ranges were validated above.
    let Some(loaded) = parse_object(unsafe { input_bytes(loaded, loaded_length) }) else {
        return STATUS_INVALID_JSON;
    };
    let (settings, was_migrated) = merge_settings(&base, &loaded);
    let Ok(json) = serde_json::to_vec(&settings) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The flag range was validated and de-overlapped above.
    unsafe { migrated.write(u32::from(was_migrated)) };
    // SAFETY: Output ranges were validated and checked for overlap above.
    unsafe { write_json(json, output, output_capacity, output_length) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_persistent_json(
    settings: *const u8,
    settings_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if let Err(status) = single_ranges_valid(
        settings,
        settings_length,
        output,
        output_capacity,
        output_length,
    ) {
        return status;
    }
    // SAFETY: The input range was validated above.
    let input = unsafe { input_bytes(settings, settings_length) };
    let Some(settings) = parse_object(input) else {
        return STATUS_INVALID_JSON;
    };
    let Ok(json) = serde_json::to_vec(&persistent_settings(&settings)) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: Output ranges were validated and checked for overlap above.
    unsafe { write_json(json, output, output_capacity, output_length) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_own_schema_and_accept_only_platform_path_arrays() {
        let defaults = default_settings(
            json!({
                "pluginPathsVST3": ["/platform/vst3"],
                "theme": "untrusted-platform-value",
                "pluginPathsLV2": "not-an-array"
            })
            .as_object()
            .expect("fixture object"),
        );
        assert_eq!(defaults.get("theme"), Some(&json!("Dark")));
        assert_eq!(
            defaults.get("pluginPathsVST3"),
            Some(&json!(["/platform/vst3"]))
        );
        assert_eq!(defaults.get("pluginPathsLV2"), Some(&json!([])));
        assert_eq!(
            defaults
                .get("shortcuts")
                .and_then(Value::as_object)
                .and_then(|shortcuts| shortcuts.get("project.save")),
            Some(&json!("Ctrl+S"))
        );
    }

    #[test]
    fn merge_preserves_defaults_and_migrates_legacy_repositories() {
        let base = default_settings(&Map::new());
        let loaded = json!({
            "theme": "Light",
            "shortcuts": {"project.save": "Meta+S"},
            "packageRepositoryUrls": ["https://example.invalid/repo.json"]
        });
        let (merged, migrated) = merge_settings(&base, loaded.as_object().expect("fixture object"));
        assert!(migrated);
        assert_eq!(merged.get("theme"), Some(&json!("Light")));
        assert_eq!(
            merged
                .get("shortcuts")
                .and_then(Value::as_object)
                .and_then(|shortcuts| shortcuts.get("project.new")),
            Some(&json!("Ctrl+N"))
        );
        assert_eq!(
            merged
                .get("shortcuts")
                .and_then(Value::as_object)
                .and_then(|shortcuts| shortcuts.get("project.save")),
            Some(&json!("Meta+S"))
        );
        assert!(!merged.contains_key("packageRepositoryUrls"));
        assert_eq!(
            merged
                .get("packageRepositories")
                .and_then(Value::as_array)
                .and_then(|repositories| repositories.first())
                .and_then(Value::as_object)
                .and_then(|repository| repository.get("name")),
            Some(&json!("https://example.invalid/repo.json"))
        );
    }

    #[test]
    fn persistent_document_removes_only_runtime_keys() {
        let settings = json!({
            "theme": "Dark",
            "_runtime": 1,
            "nested": {"_preserved": true}
        });
        let persistent = persistent_settings(settings.as_object().expect("fixture object"));
        assert!(!persistent.contains_key("_runtime"));
        assert_eq!(persistent.get("nested"), Some(&json!({"_preserved": true})));
    }
}
