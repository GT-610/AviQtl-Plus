use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid, utf8,
};
use serde_json::{Map, Value, json};
use std::sync::Mutex;

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
            let repositories: Vec<Value> = legacy
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|url| !url.is_empty())
                .map(|url| {
                    json!({
                        "url": url,
                        "name": url,
                        "enabled": true,
                        "priority": 10
                    })
                })
                .collect();
            if !repositories.is_empty() {
                settings.insert("packageRepositories".to_owned(), Value::Array(repositories));
            }
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

fn parse_value_document(input: &[u8]) -> Option<Value> {
    let mut values = serde_json::from_slice::<Vec<Value>>(input).ok()?;
    (values.len() == 1).then(|| values.remove(0))
}

fn value_as_i32(value: &Value) -> i32 {
    match value {
        Value::Bool(value) => i32::from(*value),
        Value::Number(value) => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .or_else(|| value.as_u64().and_then(|value| i32::try_from(value).ok()))
            .or_else(|| {
                value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .map(|value| value as i32)
            })
            .unwrap_or(0),
        Value::String(value) => value.parse().unwrap_or(0),
        _ => 0,
    }
}

fn value_as_f64(value: &Value) -> f64 {
    match value {
        Value::Bool(value) => f64::from(u8::from(*value)),
        Value::Number(value) => value.as_f64().unwrap_or(0.0),
        Value::String(value) => value.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn value_as_bool(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        }
        _ => false,
    }
}

pub struct AviQtlSettingsState {
    settings: Mutex<Map<String, Value>>,
}

fn with_state<T>(
    handle: *const AviQtlSettingsState,
    operation: impl FnOnce(&Map<String, Value>) -> T,
) -> Option<T> {
    // SAFETY: Non-null handles are required to originate from `aviqtl_settings_state_create`.
    let state = unsafe { handle.as_ref() }?;
    let guard = state.settings.lock().ok()?;
    Some(operation(&guard))
}

fn with_state_mut<T>(
    handle: *mut AviQtlSettingsState,
    operation: impl FnOnce(&mut Map<String, Value>) -> T,
) -> Option<T> {
    // SAFETY: Non-null handles are required to originate from `aviqtl_settings_state_create`.
    let state = unsafe { handle.as_ref() }?;
    let mut guard = state.settings.lock().ok()?;
    Some(operation(&mut guard))
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

fn output_ranges_valid(
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> Result<(), u32> {
    if !slice_is_valid(output, output_capacity) || !slice_is_valid(output_length, 1) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    match ranges_overlap(output, output_capacity, output_length, 1) {
        Some(false) => Ok(()),
        Some(true) => Err(STATUS_OVERLAPPING_BUFFERS),
        None => Err(STATUS_INVALID_ARGUMENT),
    }
}

/// Creates Rust-owned settings state from a JSON object.
///
/// # Safety
///
/// The input range must be readable UTF-8 JSON. `output_handle` must point to writable storage
/// for one handle and may not overlap the input. A returned handle must be destroyed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_create(
    input: *const u8,
    input_length: usize,
    output_handle: *mut *mut AviQtlSettingsState,
) -> u32 {
    if !slice_is_valid(input, input_length) || !slice_is_valid(output_handle, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(input, input_length, output_handle, 1) {
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        Some(false) => {}
        None => return STATUS_INVALID_ARGUMENT,
    }
    // SAFETY: The input range was validated above.
    let Some(settings) = parse_object(unsafe { input_bytes(input, input_length) }) else {
        return STATUS_INVALID_JSON;
    };
    let handle = Box::into_raw(Box::new(AviQtlSettingsState {
        settings: Mutex::new(settings),
    }));
    // SAFETY: The output handle was validated and does not overlap the input.
    unsafe { output_handle.write(handle) };
    STATUS_OK
}

/// Destroys Rust-owned settings state. A null handle is accepted.
///
/// # Safety
///
/// A non-null handle must have been returned by `aviqtl_settings_state_create` exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_destroy(handle: *mut AviQtlSettingsState) {
    if !handle.is_null() {
        // SAFETY: The caller guarantees unique ownership of one live handle.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Atomically replaces the settings document.
///
/// # Safety
///
/// The handle must be live and the input range must remain readable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_reset(
    handle: *mut AviQtlSettingsState,
    input: *const u8,
    input_length: usize,
) -> u32 {
    if handle.is_null() || !slice_is_valid(input, input_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The input range was validated above.
    let Some(replacement) = parse_object(unsafe { input_bytes(input, input_length) }) else {
        return STATUS_INVALID_JSON;
    };
    with_state_mut(handle, |settings| *settings = replacement)
        .map(|()| STATUS_OK)
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Merges a loaded settings JSON object into the current state atomically.
///
/// # Safety
///
/// The handle and loaded byte range must be valid. `migrated` must be writable and may not
/// overlap the loaded range. It is written only after validation and successful parsing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_merge_json(
    handle: *mut AviQtlSettingsState,
    loaded: *const u8,
    loaded_length: usize,
    migrated: *mut u32,
) -> u32 {
    if handle.is_null() || !slice_is_valid(loaded, loaded_length) || !slice_is_valid(migrated, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(loaded, loaded_length, migrated, 1) {
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        Some(false) => {}
        None => return STATUS_INVALID_ARGUMENT,
    }
    // SAFETY: The loaded range was validated above.
    let Some(loaded) = parse_object(unsafe { input_bytes(loaded, loaded_length) }) else {
        return STATUS_INVALID_JSON;
    };
    let Some(was_migrated) = with_state_mut(handle, |settings| {
        let (merged, migrated) = merge_settings(settings, &loaded);
        *settings = merged;
        migrated
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The output flag was validated and checked against the loaded range.
    unsafe { migrated.write(u32::from(was_migrated)) };
    STATUS_OK
}

unsafe fn state_json(
    handle: *const AviQtlSettingsState,
    persistent: bool,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if handle.is_null() {
        return STATUS_INVALID_ARGUMENT;
    }
    if let Err(status) = output_ranges_valid(output, output_capacity, output_length) {
        return status;
    }
    with_state(handle, |settings| {
        let document = if persistent {
            persistent_settings(settings)
        } else {
            settings.clone()
        };
        let Ok(json) = serde_json::to_vec(&document) else {
            return STATUS_INVALID_JSON;
        };
        // SAFETY: Output ranges were validated and checked for overlap above.
        unsafe { write_json(json, output, output_capacity, output_length) }
    })
    .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Serializes the complete settings state.
///
/// # Safety
///
/// The handle must be live. Output ranges must be writable, valid, and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_snapshot_json(
    handle: *const AviQtlSettingsState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: This function forwards the caller's state/output contract unchanged.
    unsafe { state_json(handle, false, output, output_capacity, output_length) }
}

/// Serializes settings state without runtime keys whose names begin with `_`.
///
/// # Safety
///
/// The handle must be live. Output ranges must be writable, valid, and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_persistent_json(
    handle: *const AviQtlSettingsState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: This function forwards the caller's state/output contract unchanged.
    unsafe { state_json(handle, true, output, output_capacity, output_length) }
}

/// Sets one settings entry from a single-element JSON array and reports mutation policy.
///
/// # Safety
///
/// The handle, key, and value-document ranges must be valid. `changed` and `persistent` must be
/// writable, pairwise disjoint, and disjoint from both input ranges.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_set_value_json(
    handle: *mut AviQtlSettingsState,
    key: *const u8,
    key_length: usize,
    value_document: *const u8,
    value_document_length: usize,
    changed: *mut u32,
    persistent: *mut u32,
) -> u32 {
    if handle.is_null()
        || !slice_is_valid(value_document, value_document_length)
        || !slice_is_valid(changed, 1)
        || !slice_is_valid(persistent, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: `utf8` validates the key range before decoding it.
    let Some(key) = (unsafe { utf8(key, key_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let overlaps = [
        ranges_overlap(
            key.as_ptr(),
            key.len(),
            value_document,
            value_document_length,
        ),
        ranges_overlap(key.as_ptr(), key.len(), changed, 1),
        ranges_overlap(key.as_ptr(), key.len(), persistent, 1),
        ranges_overlap(value_document, value_document_length, changed, 1),
        ranges_overlap(value_document, value_document_length, persistent, 1),
        ranges_overlap(changed, 1, persistent, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    // SAFETY: The value-document byte range was validated above.
    let Some(value) =
        parse_value_document(unsafe { input_bytes(value_document, value_document_length) })
    else {
        return STATUS_INVALID_JSON;
    };
    let Some(was_changed) = with_state_mut(handle, |settings| {
        if settings.get(key) == Some(&value) {
            false
        } else {
            settings.insert(key.to_owned(), value);
            true
        }
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: Both flags were validated and checked against all input/output ranges.
    unsafe {
        changed.write(u32::from(was_changed));
        persistent.write(u32::from(!key.starts_with('_')));
    }
    STATUS_OK
}

/// Removes one settings entry and reports mutation policy.
///
/// # Safety
///
/// The handle and key range must be valid. `changed` and `persistent` must be writable and
/// pairwise disjoint from each other and the key input range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_remove_value(
    handle: *mut AviQtlSettingsState,
    key: *const u8,
    key_length: usize,
    changed: *mut u32,
    persistent: *mut u32,
) -> u32 {
    if handle.is_null() || !slice_is_valid(changed, 1) || !slice_is_valid(persistent, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: `utf8` validates the key range before decoding it.
    let Some(key) = (unsafe { utf8(key, key_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let overlaps = [
        ranges_overlap(key.as_ptr(), key.len(), changed, 1),
        ranges_overlap(key.as_ptr(), key.len(), persistent, 1),
        ranges_overlap(changed, 1, persistent, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let Some(was_changed) = with_state_mut(handle, |settings| settings.remove(key).is_some())
    else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: Both flags were validated and checked against the input and each other.
    unsafe {
        changed.write(u32::from(was_changed));
        persistent.write(u32::from(!key.starts_with('_')));
    }
    STATUS_OK
}

/// Reads one setting using QVariant-compatible integer coercion and a missing-key fallback.
///
/// # Safety
///
/// The handle must be live and the key range must contain valid UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_get_i32(
    handle: *const AviQtlSettingsState,
    key: *const u8,
    key_length: usize,
    fallback: i32,
) -> i32 {
    // SAFETY: `utf8` validates the key range before decoding it.
    let Some(key) = (unsafe { utf8(key, key_length) }) else {
        return fallback;
    };
    with_state(handle, |settings| {
        settings.get(key).map(value_as_i32).unwrap_or(fallback)
    })
    .unwrap_or(fallback)
}

/// Reads one setting using QVariant-compatible floating-point coercion and a missing-key fallback.
///
/// # Safety
///
/// The handle must be live and the key range must contain valid UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_get_f64(
    handle: *const AviQtlSettingsState,
    key: *const u8,
    key_length: usize,
    fallback: f64,
) -> f64 {
    // SAFETY: `utf8` validates the key range before decoding it.
    let Some(key) = (unsafe { utf8(key, key_length) }) else {
        return fallback;
    };
    with_state(handle, |settings| {
        settings.get(key).map(value_as_f64).unwrap_or(fallback)
    })
    .unwrap_or(fallback)
}

/// Reads one setting using QVariant-compatible boolean coercion and a missing-key fallback.
///
/// # Safety
///
/// The handle must be live and the key range must contain valid UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_settings_state_get_bool(
    handle: *const AviQtlSettingsState,
    key: *const u8,
    key_length: usize,
    fallback: u32,
) -> u32 {
    // SAFETY: `utf8` validates the key range before decoding it.
    let Some(key) = (unsafe { utf8(key, key_length) }) else {
        return u32::from(fallback != 0);
    };
    with_state(handle, |settings| {
        settings
            .get(key)
            .map(value_as_bool)
            .unwrap_or(fallback != 0)
    })
    .map(u32::from)
    .unwrap_or_else(|| u32::from(fallback != 0))
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

        for invalid in [json!("not-an-array"), json!([]), json!([""])] {
            let loaded = json!({"packageRepositoryUrls": invalid});
            let (merged, migrated) =
                merge_settings(&base, loaded.as_object().expect("fixture object"));
            assert!(migrated);
            assert_eq!(
                merged.get("packageRepositories"),
                base.get("packageRepositories")
            );
            assert!(!merged.contains_key("packageRepositoryUrls"));
        }
    }

    #[test]
    fn rust_defaults_match_cpp_constants() {
        let constants = include_str!("../../../core/include/constants.hpp");
        for (name, value) in [
            ("kDefaultWidth", DEFAULT_WIDTH.to_string()),
            ("kDefaultHeight", DEFAULT_HEIGHT.to_string()),
            ("kDefaultSampleRate", DEFAULT_SAMPLE_RATE.to_string()),
            ("kDefaultClipDuration", DEFAULT_CLIP_DURATION.to_string()),
            ("kAudioMaxBlockSize", AUDIO_MAX_BLOCK_SIZE.to_string()),
        ] {
            assert!(
                constants.contains(&format!("constexpr int {name} = {value};")),
                "C++ constant {name} differs from the Rust default"
            );
        }
        assert!(constants.contains(&format!("constexpr double kDefaultFps = {DEFAULT_FPS:.1};")));
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

    #[test]
    fn typed_value_coercion_matches_runtime_settings_expectations() {
        for (value, expected) in [
            (json!(42), 42),
            (json!(42.9), 42),
            (json!(true), 1),
            (json!("17"), 17),
            (json!("invalid"), 0),
        ] {
            assert_eq!(value_as_i32(&value), expected);
        }
        assert_eq!(value_as_f64(&json!(1.25)), 1.25);
        assert_eq!(value_as_f64(&json!("2.5")), 2.5);
        assert_eq!(value_as_f64(&json!("invalid")), 0.0);
        assert!(!value_as_bool(&json!(false)));
        assert!(!value_as_bool(&json!(0)));
        assert!(!value_as_bool(&json!("false")));
        assert!(value_as_bool(&json!("enabled")));
    }

    unsafe fn state_snapshot(handle: *const AviQtlSettingsState, persistent: bool) -> Value {
        let mut required = 0;
        let query = if persistent {
            // SAFETY: The live test handle and zero-capacity size query satisfy the contract.
            unsafe {
                aviqtl_settings_state_persistent_json(
                    handle,
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                )
            }
        } else {
            // SAFETY: The live test handle and zero-capacity size query satisfy the contract.
            unsafe {
                aviqtl_settings_state_snapshot_json(handle, std::ptr::null_mut(), 0, &mut required)
            }
        };
        assert_eq!(query, STATUS_BUFFER_TOO_SMALL);
        let mut output = vec![0_u8; required];
        let mut written = 0;
        let status = if persistent {
            // SAFETY: The output and length ranges are writable, valid, and disjoint.
            unsafe {
                aviqtl_settings_state_persistent_json(
                    handle,
                    output.as_mut_ptr(),
                    output.len(),
                    &mut written,
                )
            }
        } else {
            // SAFETY: The output and length ranges are writable, valid, and disjoint.
            unsafe {
                aviqtl_settings_state_snapshot_json(
                    handle,
                    output.as_mut_ptr(),
                    output.len(),
                    &mut written,
                )
            }
        };
        assert_eq!(status, STATUS_OK);
        assert_eq!(written, output.len());
        serde_json::from_slice(&output).expect("settings snapshot JSON")
    }

    #[test]
    fn settings_state_owns_mutation_merge_typed_reads_and_persistence() {
        let initial = br#"{"theme":"Dark","_runtime":1,"count":3}"#;
        let mut handle = std::ptr::null_mut();
        // SAFETY: All byte and handle-output ranges are valid and disjoint.
        assert_eq!(
            unsafe { aviqtl_settings_state_create(initial.as_ptr(), initial.len(), &mut handle) },
            STATUS_OK
        );
        assert!(!handle.is_null());

        // SAFETY: The live handle and UTF-8 key ranges satisfy the typed-read contracts.
        unsafe {
            assert_eq!(
                aviqtl_settings_state_get_i32(handle, b"count".as_ptr(), 5, -1),
                3
            );
            assert_eq!(
                aviqtl_settings_state_get_i32(handle, b"missing".as_ptr(), 7, 9),
                9
            );
        }

        let value = b"[42]";
        let mut changed = 99;
        let mut persistent = 99;
        // SAFETY: The live handle, UTF-8 key, JSON value document, and flags are disjoint.
        assert_eq!(
            unsafe {
                aviqtl_settings_state_set_value_json(
                    handle,
                    b"answer".as_ptr(),
                    6,
                    value.as_ptr(),
                    value.len(),
                    &mut changed,
                    &mut persistent,
                )
            },
            STATUS_OK
        );
        assert_eq!(changed, 1);
        assert_eq!(persistent, 1);
        // An identical value is a semantic no-op.
        assert_eq!(
            unsafe {
                aviqtl_settings_state_set_value_json(
                    handle,
                    b"answer".as_ptr(),
                    6,
                    value.as_ptr(),
                    value.len(),
                    &mut changed,
                    &mut persistent,
                )
            },
            STATUS_OK
        );
        assert_eq!(changed, 0);

        let runtime_value = b"[true]";
        assert_eq!(
            unsafe {
                aviqtl_settings_state_set_value_json(
                    handle,
                    b"_temporary".as_ptr(),
                    10,
                    runtime_value.as_ptr(),
                    runtime_value.len(),
                    &mut changed,
                    &mut persistent,
                )
            },
            STATUS_OK
        );
        assert_eq!(persistent, 0);

        let loaded = br#"{"theme":"Light","packageRepositoryUrls":["https://example.invalid"]}"#;
        let mut migrated = 0;
        // SAFETY: The loaded JSON and migration output are valid and disjoint.
        assert_eq!(
            unsafe {
                aviqtl_settings_state_merge_json(
                    handle,
                    loaded.as_ptr(),
                    loaded.len(),
                    &mut migrated,
                )
            },
            STATUS_OK
        );
        assert_eq!(migrated, 1);
        // SAFETY: `handle` remains live for both snapshot calls.
        let complete = unsafe { state_snapshot(handle, false) };
        assert_eq!(complete.get("theme"), Some(&json!("Light")));
        assert_eq!(complete.get("answer"), Some(&json!(42)));
        assert!(
            !complete
                .as_object()
                .expect("settings object")
                .contains_key("packageRepositoryUrls")
        );
        // SAFETY: `handle` remains live for both snapshot calls.
        let persisted = unsafe { state_snapshot(handle, true) };
        assert!(persisted.get("_temporary").is_none());
        assert_eq!(persisted.get("answer"), Some(&json!(42)));

        // SAFETY: The handle was returned by create and has not yet been destroyed.
        unsafe { aviqtl_settings_state_destroy(handle) };
    }

    #[test]
    fn settings_state_rejects_invalid_updates_without_partial_state_or_flags() {
        let initial = br#"{"value":1}"#;
        let mut handle = std::ptr::null_mut();
        // SAFETY: All byte and handle-output ranges are valid and disjoint.
        assert_eq!(
            unsafe { aviqtl_settings_state_create(initial.as_ptr(), initial.len(), &mut handle) },
            STATUS_OK
        );
        let invalid = b"not-json";
        let mut changed = 77;
        let mut persistent = 88;
        // SAFETY: All ranges are valid and disjoint; invalid JSON must be rejected atomically.
        assert_eq!(
            unsafe {
                aviqtl_settings_state_set_value_json(
                    handle,
                    b"value".as_ptr(),
                    5,
                    invalid.as_ptr(),
                    invalid.len(),
                    &mut changed,
                    &mut persistent,
                )
            },
            STATUS_INVALID_JSON
        );
        assert_eq!(changed, 77);
        assert_eq!(persistent, 88);
        // SAFETY: `handle` is still live after the rejected mutation.
        assert_eq!(
            unsafe { state_snapshot(handle, false) },
            json!({"value": 1})
        );

        // SAFETY: The handle was returned by create and has not yet been destroyed.
        unsafe { aviqtl_settings_state_destroy(handle) };
    }
}
