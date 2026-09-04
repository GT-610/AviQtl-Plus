use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use crate::package::compare_versions;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::sync::Mutex;

const MANIFEST_ID_LIMIT: usize = 255;
const MANIFEST_NAME_LIMIT: usize = 255;
const MANIFEST_VERSION_LIMIT: usize = 64;
const MANIFEST_MIN_APP_VERSION_LIMIT: usize = 64;

const MANIFEST_FIELDS: [&str; 6] = [
    "id",
    "name",
    "version",
    "author",
    "description",
    "minAppVersion",
];

#[derive(Default)]
struct ScriptPluginCatalog {
    plugins: Vec<Map<String, Value>>,
}

impl ScriptPluginCatalog {
    fn identity(plugin: &Map<String, Value>) -> Option<(&str, &str)> {
        let id = plugin.get("manifest")?.as_object()?.get("id")?.as_str()?;
        let path = plugin.get("filePath")?.as_str()?;
        (!id.is_empty() && !path.is_empty()).then_some((id, path))
    }

    fn store(&mut self, plugin: Map<String, Value>) -> bool {
        let Some((id, path)) = Self::identity(&plugin) else {
            return false;
        };
        if let Some(index) = self.plugins.iter().position(|existing| {
            Self::identity(existing).is_some_and(|(existing_id, _)| existing_id == id)
        }) {
            let same_path = Self::identity(&self.plugins[index])
                .is_some_and(|(_, existing_path)| existing_path == path);
            if !same_path {
                return false;
            }
            self.plugins[index] = plugin;
            return true;
        }
        if self.plugins.iter().any(|existing| {
            Self::identity(existing).is_some_and(|(_, existing_path)| existing_path == path)
        }) {
            return false;
        }
        self.plugins.push(plugin);
        true
    }

    fn find(&self, id: &str) -> Map<String, Value> {
        self.plugins
            .iter()
            .find(|plugin| Self::identity(plugin).is_some_and(|(existing_id, _)| existing_id == id))
            .cloned()
            .unwrap_or_default()
    }
}

pub struct AviQtlScriptPluginCatalogState {
    catalog: Mutex<ScriptPluginCatalog>,
}

fn text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn integer(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_i64).unwrap_or_default()
}

fn objects(value: Option<&Value>) -> Vec<Map<String, Value>> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .cloned()
        .collect()
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn normalize_manifest(mut manifest: Map<String, Value>) -> Map<String, Value> {
    for field in MANIFEST_FIELDS {
        manifest.insert(
            field.to_owned(),
            Value::String(text(manifest.get(field)).trim().to_owned()),
        );
    }
    manifest
}

fn manifest_shape_is_valid(manifest: &Map<String, Value>) -> bool {
    let id = text(manifest.get("id"));
    let name = text(manifest.get("name"));
    let version = text(manifest.get("version"));
    let minimum_app_version = text(manifest.get("minAppVersion"));
    !id.is_empty()
        && !name.is_empty()
        && !version.is_empty()
        && utf16_len(&id) <= MANIFEST_ID_LIMIT
        && utf16_len(&name) <= MANIFEST_NAME_LIMIT
        && utf16_len(&version) <= MANIFEST_VERSION_LIMIT
        && utf16_len(&minimum_app_version) <= MANIFEST_MIN_APP_VERSION_LIMIT
}

fn validate_manifest(
    manifest: Map<String, Value>,
    single_file: bool,
    expected_id: &str,
    app_version: &str,
    path_identity: &str,
    loaded: &[Map<String, Value>],
) -> (Map<String, Value>, &'static str) {
    let manifest = normalize_manifest(manifest);
    if !manifest_shape_is_valid(&manifest) {
        return (manifest, "invalid_manifest");
    }
    let id = text(manifest.get("id"));
    let valid_identity = if single_file {
        !expected_id.is_empty() && id == expected_id
    } else {
        crate::policy::valid_package_id(&id) && !id.starts_with("file:")
    };
    if !valid_identity {
        return (manifest, "invalid_id");
    }
    let minimum_app_version = text(manifest.get("minAppVersion"));
    if !minimum_app_version.is_empty() && compare_versions(app_version, &minimum_app_version) < 0 {
        return (manifest, "requires_newer_app");
    }
    if loaded.iter().any(|plugin| {
        text(plugin.get("id")) == id
            || (!path_identity.is_empty() && text(plugin.get("path")) == path_identity)
    }) {
        return (manifest, "duplicate");
    }
    (manifest, "ok")
}

fn normalize_category(category: &str) -> String {
    let category = category.trim();
    let lower = category.to_lowercase();
    match lower.as_str() {
        "synth" | "instrument" => "Synth".to_owned(),
        "delay" | "reverb" => "Delay".to_owned(),
        "eq" => "EQ".to_owned(),
        "filter" => "Filter".to_owned(),
        "distortion" => "Distortion".to_owned(),
        "dynamics" => "Dynamics".to_owned(),
        "modulator" | "modulation" => "Modulator".to_owned(),
        "utility" | "tools" | "tool" => "Utility".to_owned(),
        "" | "other" | "unknown" | "misc" | "none" | "null" => "Other".to_owned(),
        _ => {
            let mut characters = lower.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => "Other".to_owned(),
            }
        }
    }
}

fn carla_category(category: i32) -> &'static str {
    match category {
        1 => "Synth",
        2 => "Delay",
        3 => "EQ",
        4 => "Filter",
        5 => "Distortion",
        6 => "Dynamics",
        7 => "Modulator",
        8 => "Utility",
        _ => "Other",
    }
}

fn category_rank(category: &str) -> i32 {
    match normalize_category(category).as_str() {
        "Filter" => 0,
        "EQ" => 1,
        "Dynamics" => 2,
        "Delay" => 3,
        "Distortion" => 4,
        "Modulator" => 5,
        "Utility" => 6,
        "Synth" => 7,
        _ => 100,
    }
}

fn normalize_plugin(mut plugin: Map<String, Value>, fallback_name: &str) -> Map<String, Value> {
    let label = text(plugin.get("label")).trim().to_owned();
    let mut name = text(plugin.get("name")).trim().to_owned();
    if name.is_empty() {
        name = if label.is_empty() {
            fallback_name.trim().to_owned()
        } else {
            label.clone()
        };
    }
    let label = if label.is_empty() {
        name.clone()
    } else {
        label
    };
    let category = normalize_category(&text(plugin.get("category")));
    plugin.insert("name".to_owned(), Value::String(name));
    plugin.insert("label".to_owned(), Value::String(label));
    plugin.insert("category".to_owned(), Value::String(category));
    plugin
}

fn finish_discovery_plugin(
    mut plugin: Map<String, Value>,
    fallback_name: &str,
) -> Option<Map<String, Value>> {
    plugin = normalize_plugin(plugin, fallback_name);
    let name = text(plugin.get("name"));
    if name.is_empty() {
        return None;
    }
    let format = text(plugin.get("format"));
    let label = text(plugin.get("label"));
    let unique_id = integer(plugin.get("uniqueId"));
    plugin.insert(
        "id".to_owned(),
        Value::String(format!("{format}:{label}:{unique_id}")),
    );
    Some(plugin)
}

fn parse_discovery_output(
    output: &str,
    format: &str,
    file_path: &str,
    fallback_name: &str,
) -> Vec<Map<String, Value>> {
    let mut plugins = Vec::new();
    let mut current = Map::new();
    let mut in_block = false;
    for raw_line in output.lines() {
        let line = raw_line.trim();
        let Some(payload) = line.strip_prefix("carla-discovery::") else {
            continue;
        };
        let mut parts = payload.split("::");
        let key = parts.next().unwrap_or_default();
        let value = parts.collect::<Vec<_>>().join("::");
        match key {
            "begin" | "init" => {
                current = Map::new();
                current.insert("format".to_owned(), Value::String(format.to_owned()));
                current.insert("path".to_owned(), Value::String(file_path.to_owned()));
                current.insert("uniqueId".to_owned(), Value::from(0));
                current.insert("index".to_owned(), Value::from(0));
                current.insert("audioIns".to_owned(), Value::from(0));
                current.insert("audioOuts".to_owned(), Value::from(0));
                in_block = true;
            }
            _ if !in_block => {}
            "name" | "label" | "maker" => {
                current.insert(key.to_owned(), Value::String(value));
            }
            "uniqueId" => {
                current.insert(
                    "uniqueId".to_owned(),
                    Value::from(value.trim().parse::<i64>().unwrap_or_default()),
                );
            }
            "category" => {
                let trimmed = value.trim();
                let category = trimmed
                    .parse::<i32>()
                    .ok()
                    .map(carla_category)
                    .unwrap_or(trimmed);
                current.insert(
                    "category".to_owned(),
                    Value::String(normalize_category(category)),
                );
            }
            "audio.ins" => {
                current.insert(
                    "audioIns".to_owned(),
                    Value::from(value.trim().parse::<i32>().unwrap_or_default()),
                );
            }
            "audio.outs" => {
                current.insert(
                    "audioOuts".to_owned(),
                    Value::from(value.trim().parse::<i32>().unwrap_or_default()),
                );
            }
            "end" => {
                if let Some(plugin) = finish_discovery_plugin(current, fallback_name) {
                    plugins.push(plugin);
                }
                current = Map::new();
                in_block = false;
            }
            _ => {}
        }
    }
    plugins
}

fn deduplicate(plugins: Vec<Map<String, Value>>) -> Vec<Map<String, Value>> {
    let mut ids = BTreeSet::new();
    plugins
        .into_iter()
        .filter(|plugin| {
            let id = text(plugin.get("id"));
            !id.is_empty() && ids.insert(id)
        })
        .collect()
}

fn public_plugin(plugin: &Map<String, Value>, include_io: bool) -> Map<String, Value> {
    let mut result = Map::new();
    for key in ["id", "name", "format", "category", "maker"] {
        result.insert(key.to_owned(), plugin.get(key).cloned().unwrap_or_default());
    }
    if include_io {
        result.insert(
            "audioIns".to_owned(),
            plugin.get("audioIns").cloned().unwrap_or(Value::from(0)),
        );
        result.insert(
            "audioOuts".to_owned(),
            plugin.get("audioOuts").cloned().unwrap_or(Value::from(0)),
        );
    }
    result
}

fn categories(plugins: &[Map<String, Value>]) -> Vec<Value> {
    let mut categories = BTreeSet::new();
    for plugin in plugins {
        categories.insert(normalize_category(&text(plugin.get("category"))));
    }
    let mut categories: Vec<_> = categories.into_iter().collect();
    categories.sort_by(|left, right| {
        category_rank(left)
            .cmp(&category_rank(right))
            .then_with(|| left.to_lowercase().cmp(&right.to_lowercase()))
    });
    categories.into_iter().map(Value::String).collect()
}

fn filtered_plugins(plugins: Vec<Map<String, Value>>, category: &str) -> Vec<Value> {
    let wanted = normalize_category(category);
    let mut plugins: Vec<_> = plugins
        .into_iter()
        .filter(|plugin| normalize_category(&text(plugin.get("category"))) == wanted)
        .collect();
    plugins.sort_by_key(|plugin| text(plugin.get("name")).to_lowercase());
    plugins
        .iter()
        .map(|plugin| Value::Object(public_plugin(plugin, false)))
        .collect()
}

fn apply(input: &Map<String, Value>) -> Option<Map<String, Value>> {
    match text(input.get("operation")).as_str() {
        "normalizeManifest" => {
            let manifest = normalize_manifest(input.get("manifest")?.as_object()?.clone());
            Some(
                json!({
                    "manifest": manifest,
                    "valid": manifest_shape_is_valid(&manifest)
                })
                .as_object()
                .cloned()?,
            )
        }
        "validateManifest" => {
            let (manifest, status) = validate_manifest(
                input.get("manifest")?.as_object()?.clone(),
                input
                    .get("singleFile")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                &text(input.get("expectedId")),
                &text(input.get("appVersion")),
                &text(input.get("pathIdentity")),
                &objects(input.get("loaded")),
            );
            Some(
                json!({"manifest": manifest, "status": status})
                    .as_object()
                    .cloned()?,
            )
        }
        "parseDiscovery" => Some(
            json!({
                "plugins": parse_discovery_output(
                    &text(input.get("output")),
                    &text(input.get("format")),
                    &text(input.get("filePath")),
                    &text(input.get("fallbackName")),
                )
            })
            .as_object()
            .cloned()?,
        ),
        "deduplicate" => Some(
            json!({"plugins": deduplicate(objects(input.get("plugins")))})
                .as_object()
                .cloned()?,
        ),
        "list" => Some(
            json!({
                "plugins": objects(input.get("plugins"))
                    .iter()
                    .map(|plugin| Value::Object(public_plugin(plugin, true)))
                    .collect::<Vec<_>>()
            })
            .as_object()
            .cloned()?,
        ),
        "categories" => Some(
            json!({"categories": categories(&objects(input.get("plugins")))})
                .as_object()
                .cloned()?,
        ),
        "filter" => Some(
            json!({
                "plugins": filtered_plugins(
                    objects(input.get("plugins")),
                    &text(input.get("category"))
                )
            })
            .as_object()
            .cloned()?,
        ),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_plugin_document_apply_json(
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(input, input_length)
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        ranges_overlap(input, input_length, output, output_capacity),
        ranges_overlap(input, input_length, output_length, 1),
        ranges_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let input = if input_length == 0 {
        &[]
    } else {
        // SAFETY: The input range was validated and checked against every output.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    let Some(input) = serde_json::from_slice::<Value>(input)
        .ok()
        .and_then(|value| value.as_object().cloned())
    else {
        return STATUS_INVALID_JSON;
    };
    let Some(result) = apply(&input) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Ok(json) = serde_json::to_vec(&result) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The output length was validated and checked against the other ranges.
    unsafe { output_length.write(json.len()) };
    if output_capacity < json.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !json.is_empty() {
        // SAFETY: The output range has sufficient capacity and is disjoint from the inputs.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..json.len()].copy_from_slice(&json);
    }
    STATUS_OK
}

fn with_script_catalog<T>(
    handle: *const AviQtlScriptPluginCatalogState,
    function: impl FnOnce(&ScriptPluginCatalog) -> T,
) -> Option<T> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: The caller supplies a live handle for the duration of this call.
    let handle = unsafe { &*handle };
    handle.catalog.lock().ok().map(|catalog| function(&catalog))
}

fn with_script_catalog_mut<T>(
    handle: *mut AviQtlScriptPluginCatalogState,
    function: impl FnOnce(&mut ScriptPluginCatalog) -> T,
) -> Option<T> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: The caller supplies a live handle for the duration of this call.
    let handle = unsafe { &*handle };
    handle
        .catalog
        .lock()
        .ok()
        .map(|mut catalog| function(&mut catalog))
}

unsafe fn input_bytes<'a>(input: *const u8, input_length: usize) -> &'a [u8] {
    if input_length == 0 {
        &[]
    } else {
        // SAFETY: The caller validated the readable input range.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    }
}

fn output_ranges_valid(
    inputs: &[(*const u8, usize)],
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> Result<(), u32> {
    if !slice_is_valid(output, output_capacity) || !slice_is_valid(output_length, 1) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    let mut overlaps = Vec::with_capacity(inputs.len() * 2 + 1);
    overlaps.push(ranges_overlap(output, output_capacity, output_length, 1));
    for (input, input_length) in inputs {
        if !slice_is_valid(*input, *input_length) {
            return Err(STATUS_INVALID_ARGUMENT);
        }
        overlaps.push(ranges_overlap(
            *input,
            *input_length,
            output,
            output_capacity,
        ));
        overlaps.push(ranges_overlap(*input, *input_length, output_length, 1));
    }
    if overlaps.iter().any(Option::is_none) {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return Err(STATUS_OVERLAPPING_BUFFERS);
    }
    Ok(())
}

unsafe fn write_json(
    value: &Value,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let Ok(bytes) = serde_json::to_vec(value) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The output-length pointer was validated and de-overlapped by the caller.
    unsafe { output_length.write(bytes.len()) };
    if output_capacity < bytes.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !bytes.is_empty() {
        // SAFETY: The output range was validated and has sufficient capacity.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..bytes.len()].copy_from_slice(&bytes);
    }
    STATUS_OK
}

unsafe fn script_catalog_json(
    handle: *const AviQtlScriptPluginCatalogState,
    inputs: &[(*const u8, usize)],
    value: impl FnOnce(&ScriptPluginCatalog) -> Value,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if handle.is_null() {
        return STATUS_INVALID_ARGUMENT;
    }
    if let Err(status) = output_ranges_valid(inputs, output, output_capacity, output_length) {
        return status;
    }
    with_script_catalog(handle, |catalog| {
        // SAFETY: Output ranges were validated and checked for overlap above.
        unsafe { write_json(&value(catalog), output, output_capacity, output_length) }
    })
    .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Creates an empty Rust-owned script-plugin catalog.
///
/// # Safety
///
/// `output_handle` must be writable for one pointer. The returned handle must be destroyed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_script_plugin_catalog_state_create(
    output_handle: *mut *mut AviQtlScriptPluginCatalogState,
) -> u32 {
    if !slice_is_valid(output_handle, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let handle = Box::into_raw(Box::new(AviQtlScriptPluginCatalogState {
        catalog: Mutex::new(ScriptPluginCatalog::default()),
    }));
    // SAFETY: The output pointer was validated above.
    unsafe { output_handle.write(handle) };
    STATUS_OK
}

/// Destroys one Rust-owned script-plugin catalog. A null handle is accepted.
///
/// # Safety
///
/// A non-null handle must have been returned by the create function and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_script_plugin_catalog_state_destroy(
    handle: *mut AviQtlScriptPluginCatalogState,
) {
    if !handle.is_null() {
        // SAFETY: The caller guarantees unique ownership of one live handle.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Clears every script-plugin record.
///
/// # Safety
///
/// The handle must be live for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_script_plugin_catalog_state_clear(
    handle: *mut AviQtlScriptPluginCatalogState,
) -> u32 {
    with_script_catalog_mut(handle, |catalog| catalog.plugins.clear())
        .map(|()| STATUS_OK)
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Stores a script-plugin record, preserving first-registration order on replacement.
///
/// # Safety
///
/// The handle must be live and `input` must contain a readable JSON object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_script_plugin_catalog_state_store_json(
    handle: *mut AviQtlScriptPluginCatalogState,
    input: *const u8,
    input_length: usize,
) -> u32 {
    if handle.is_null() || !slice_is_valid(input, input_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The input range was validated above.
    let Some(plugin) = serde_json::from_slice::<Value>(unsafe { input_bytes(input, input_length) })
        .ok()
        .and_then(|value| value.as_object().cloned())
    else {
        return STATUS_INVALID_JSON;
    };
    with_script_catalog_mut(handle, |catalog| catalog.store(plugin))
        .map(|stored| {
            if stored {
                STATUS_OK
            } else {
                STATUS_INVALID_ARGUMENT
            }
        })
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Serializes every script-plugin record in stable first-registration order.
///
/// # Safety
///
/// The handle must be live. Output ranges must be valid, writable, and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_script_plugin_catalog_state_snapshot_json(
    handle: *const AviQtlScriptPluginCatalogState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: This forwards the caller's handle and output contract unchanged.
    unsafe {
        script_catalog_json(
            handle,
            &[],
            |catalog| Value::Array(catalog.plugins.iter().cloned().map(Value::Object).collect()),
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Serializes one script-plugin record, or an empty object when the ID is absent.
///
/// # Safety
///
/// The handle must be live, `id` readable, and output ranges valid and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_script_plugin_catalog_state_find_json(
    handle: *const AviQtlScriptPluginCatalogState,
    id: *const u8,
    id_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(id, id_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The input range was validated above.
    let Some(id_value) = std::str::from_utf8(unsafe { input_bytes(id, id_length) }).ok() else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: This forwards the validated input and caller's output contract.
    unsafe {
        script_catalog_json(
            handle,
            &[(id, id_length)],
            |catalog| Value::Object(catalog.find(id_value)),
            output,
            output_capacity,
            output_length,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rules_normalize_fields_and_match_qstring_limits() {
        let manifest = normalize_manifest(
            json!({
                "id": "  org.aviqtl.example  ",
                "name": " Example ",
                "version": " 1.2.3 ",
                "author": " Author ",
                "description": " Description ",
                "minAppVersion": " 0.5.0 "
            })
            .as_object()
            .expect("fixture")
            .clone(),
        );
        assert_eq!(text(manifest.get("id")), "org.aviqtl.example");
        assert_eq!(text(manifest.get("name")), "Example");
        assert_eq!(text(manifest.get("author")), "Author");
        assert!(manifest_shape_is_valid(&manifest));

        let mut too_long = manifest.clone();
        too_long.insert("name".to_owned(), Value::String("😀".repeat(128)));
        assert!(!manifest_shape_is_valid(&too_long));
    }

    #[test]
    fn manifest_rules_own_identity_compatibility_and_duplicate_checks() {
        let manifest = json!({
            "id": "org.aviqtl.example",
            "name": "Example",
            "version": "1.0.0",
            "minAppVersion": "0.6.0"
        })
        .as_object()
        .expect("fixture")
        .clone();
        assert_eq!(
            validate_manifest(
                manifest.clone(),
                false,
                "",
                "0.5.9",
                "/plugin/main.lua",
                &[]
            )
            .1,
            "requires_newer_app"
        );
        assert_eq!(
            validate_manifest(
                manifest.clone(),
                false,
                "",
                "0.6.0",
                "/plugin/main.lua",
                &[]
            )
            .1,
            "ok"
        );
        assert_eq!(
            validate_manifest(
                manifest.clone(),
                false,
                "",
                "0.6.0",
                "/plugin/main.lua",
                &objects(Some(
                    &json!([{"id": "org.aviqtl.example", "path": "/other/main.lua"}])
                )),
            )
            .1,
            "duplicate"
        );

        let file_manifest = json!({
            "id": "file:example.lua",
            "name": "example",
            "version": "file"
        })
        .as_object()
        .expect("fixture")
        .clone();
        assert_eq!(
            validate_manifest(
                file_manifest.clone(),
                true,
                "file:example.lua",
                "0.6.0",
                "/plugin/example.lua",
                &[],
            )
            .1,
            "ok"
        );
        assert_eq!(
            validate_manifest(
                file_manifest,
                true,
                "file:renamed.lua",
                "0.6.0",
                "/plugin/example.lua",
                &[],
            )
            .1,
            "invalid_id"
        );
    }

    #[test]
    fn discovery_parser_normalizes_complete_plugin_blocks() {
        let output = "ignored\n\
carla-discovery::init\n\
carla-discovery::name::  \n\
carla-discovery::label::Example::Stereo\n\
carla-discovery::maker::AviQtl\n\
carla-discovery::uniqueId::42\n\
carla-discovery::category::4\n\
carla-discovery::audio.ins::2\n\
carla-discovery::audio.outs::2\n\
carla-discovery::end";
        let plugins = parse_discovery_output(output, "VST3", "/tmp/example.vst3", "example");
        assert_eq!(plugins.len(), 1);
        assert_eq!(text(plugins[0].get("name")), "Example::Stereo");
        assert_eq!(text(plugins[0].get("category")), "Filter");
        assert_eq!(text(plugins[0].get("id")), "VST3:Example::Stereo:42");
        assert_eq!(integer(plugins[0].get("audioIns")), 2);

        let uncategorized = parse_discovery_output(
            "carla-discovery::init\ncarla-discovery::name::No Category\ncarla-discovery::category::0\ncarla-discovery::end",
            "VST2",
            "/tmp/no-category.so",
            "no-category",
        );
        assert_eq!(text(uncategorized[0].get("category")), "Other");
    }

    #[test]
    fn catalog_deduplication_categories_and_filtering_are_stable() {
        let plugins = objects(Some(&json!([
            {"id":"first","name":"Zulu","category":"reverb","format":"LV2"},
            {"id":"second","name":"Alpha","category":"delay","format":"VST3"},
            {"id":"first","name":"Duplicate","category":"filter","format":"VST2"},
            {"id":"third","name":"EQ","category":"eq","format":"LV2"},
            {"id":"fourth","name":"Custom One","category":"Custom","format":"LV2"},
            {"id":"fifth","name":"Custom Two","category":"custom","format":"LV2"}
        ])));
        let plugins = deduplicate(plugins);
        assert_eq!(plugins.len(), 5);
        assert_eq!(
            categories(&plugins),
            vec![json!("EQ"), json!("Delay"), json!("Custom")]
        );
        let filtered = filtered_plugins(plugins.clone(), "Delay");
        assert_eq!(
            filtered
                .iter()
                .filter_map(|plugin| plugin.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>(),
            ["Alpha", "Zulu"]
        );
        assert_eq!(filtered_plugins(plugins, "CUSTOM").len(), 2);
    }

    #[test]
    fn script_plugin_catalog_owns_order_replacement_lookup_and_clear() {
        let mut catalog = ScriptPluginCatalog::default();
        let alpha = json!({
            "manifest":{"id":"plugin.alpha","name":"Alpha"},
            "filePath":"/plugins/alpha/main.lua",
            "paramValues":{"amount":1}
        })
        .as_object()
        .expect("fixture")
        .clone();
        let beta = json!({
            "manifest":{"id":"plugin.beta","name":"Beta"},
            "filePath":"/plugins/beta/main.lua",
            "paramValues":{}
        })
        .as_object()
        .expect("fixture")
        .clone();
        assert!(catalog.store(alpha.clone()));
        assert!(catalog.store(beta));

        let mut updated = alpha;
        updated.insert("paramValues".to_owned(), json!({"amount":2}));
        assert!(catalog.store(updated));
        assert_eq!(catalog.plugins.len(), 2);
        assert_eq!(catalog.plugins[0]["manifest"]["id"], "plugin.alpha");
        assert_eq!(catalog.find("plugin.alpha")["paramValues"]["amount"], 2);
        assert!(catalog.find("missing").is_empty());

        let duplicate_path = json!({
            "manifest":{"id":"plugin.other"},
            "filePath":"/plugins/alpha/main.lua"
        })
        .as_object()
        .expect("fixture")
        .clone();
        assert!(!catalog.store(duplicate_path));
        catalog.plugins.clear();
        assert!(catalog.plugins.is_empty());
    }
}
