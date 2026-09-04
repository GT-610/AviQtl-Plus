use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Default)]
struct EffectCatalog {
    effects: BTreeMap<String, Map<String, Value>>,
    ordered_ids: Vec<String>,
}

impl EffectCatalog {
    fn register(&mut self, metadata: Map<String, Value>) -> bool {
        let Some(id) = string(metadata.get("id")).filter(|id| !id.is_empty()) else {
            return false;
        };
        let id = id.to_owned();
        if !self.effects.contains_key(&id) {
            self.ordered_ids.push(id.clone());
        }
        self.effects.insert(id, metadata);
        true
    }

    fn find(&self, id: &str) -> Map<String, Value> {
        self.effects.get(id).cloned().unwrap_or_default()
    }

    fn snapshot(&self) -> Vec<Value> {
        self.ordered_ids
            .iter()
            .filter_map(|id| self.effects.get(id).cloned())
            .map(Value::Object)
            .collect()
    }

    fn remove_ids(&mut self, ids: &[Value]) {
        let removed: std::collections::BTreeSet<&str> =
            ids.iter().filter_map(Value::as_str).collect();
        self.effects.retain(|id, _| !removed.contains(id.as_str()));
        self.ordered_ids.retain(|id| !removed.contains(id.as_str()));
    }
}

pub struct AviQtlEffectCatalogState {
    catalog: Mutex<EffectCatalog>,
}

fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

fn valid_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let valid_part =
        |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let valid = parts.by_ref().take(3).all(valid_part);
    valid && parts.next().is_none() && version.split('.').count() == 3
}

fn normalize_metadata(input: &[u8]) -> Option<Map<String, Value>> {
    let mut metadata = serde_json::from_slice::<Value>(input)
        .ok()?
        .as_object()
        .cloned()?;
    let id = string(metadata.get("id"))?;
    let name = string(metadata.get("name"))?;
    let qml = string(metadata.get("qml"))?;
    if id.is_empty() || name.is_empty() || qml.is_empty() {
        return None;
    }
    let version = string(metadata.get("version"))?;
    if !valid_version(version) {
        return None;
    }
    if !matches!(string(metadata.get("kind")), Some("effect" | "object")) {
        return None;
    }
    let categories: Vec<Value> = metadata
        .get("categories")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(|category| Value::String(category.to_owned()))
        .collect();
    if categories.is_empty() {
        return None;
    }
    let ui = metadata.get("ui")?.as_object()?.clone();
    if !ui.contains_key("controls") {
        return None;
    }
    let params = metadata
        .get("params")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    metadata.insert("categories".to_owned(), Value::Array(categories));
    metadata.insert("params".to_owned(), Value::Object(params));
    metadata.insert("ui".to_owned(), Value::Object(ui));
    Some(metadata)
}

fn with_catalog<T>(
    handle: *const AviQtlEffectCatalogState,
    function: impl FnOnce(&EffectCatalog) -> T,
) -> Option<T> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: The caller supplies a live handle for the duration of this call.
    let handle = unsafe { &*handle };
    handle.catalog.lock().ok().map(|catalog| function(&catalog))
}

fn with_catalog_mut<T>(
    handle: *mut AviQtlEffectCatalogState,
    function: impl FnOnce(&mut EffectCatalog) -> T,
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

unsafe fn catalog_json(
    handle: *const AviQtlEffectCatalogState,
    inputs: &[(*const u8, usize)],
    value: impl FnOnce(&EffectCatalog) -> Value,
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
    with_catalog(handle, |catalog| {
        // SAFETY: Output ranges were validated and checked for overlap above.
        unsafe { write_json(&value(catalog), output, output_capacity, output_length) }
    })
    .unwrap_or(STATUS_INVALID_ARGUMENT)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_effect_metadata_normalize_json(
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
        // SAFETY: The input range was validated and checked for overlap.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    let Some(metadata) = normalize_metadata(input) else {
        return STATUS_INVALID_JSON;
    };
    let Ok(json) = serde_json::to_vec(&metadata) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The output-length pointer was validated and de-overlapped above.
    unsafe { output_length.write(json.len()) };
    if output_capacity < json.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !json.is_empty() {
        // SAFETY: The output range was validated and has sufficient capacity.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..json.len()].copy_from_slice(&json);
    }
    STATUS_OK
}

/// Creates an empty Rust-owned effect catalog.
///
/// # Safety
///
/// `output_handle` must be writable for one pointer. The returned handle must be destroyed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_effect_catalog_state_create(
    output_handle: *mut *mut AviQtlEffectCatalogState,
) -> u32 {
    if !slice_is_valid(output_handle, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let handle = Box::into_raw(Box::new(AviQtlEffectCatalogState {
        catalog: Mutex::new(EffectCatalog::default()),
    }));
    // SAFETY: The output pointer was validated above.
    unsafe { output_handle.write(handle) };
    STATUS_OK
}

/// Destroys one Rust-owned effect catalog. A null handle is accepted.
///
/// # Safety
///
/// A non-null handle must have been returned by the create function and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_effect_catalog_state_destroy(
    handle: *mut AviQtlEffectCatalogState,
) {
    if !handle.is_null() {
        // SAFETY: The caller guarantees unique ownership of one live handle.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Registers or replaces one effect metadata object while preserving first-registration order.
///
/// # Safety
///
/// The handle must be live and `input` must contain a readable JSON object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_effect_catalog_state_register_json(
    handle: *mut AviQtlEffectCatalogState,
    input: *const u8,
    input_length: usize,
) -> u32 {
    if handle.is_null() || !slice_is_valid(input, input_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The input range was validated above.
    let Some(metadata) =
        serde_json::from_slice::<Value>(unsafe { input_bytes(input, input_length) })
            .ok()
            .and_then(|value| value.as_object().cloned())
    else {
        return STATUS_INVALID_JSON;
    };
    with_catalog_mut(handle, |catalog| catalog.register(metadata))
        .map(|registered| {
            if registered {
                STATUS_OK
            } else {
                STATUS_INVALID_ARGUMENT
            }
        })
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Serializes all effect metadata in stable first-registration order.
///
/// # Safety
///
/// The handle must be live. Output ranges must be valid, writable, and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_effect_catalog_state_snapshot_json(
    handle: *const AviQtlEffectCatalogState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: This forwards the caller's handle and output contract unchanged.
    unsafe {
        catalog_json(
            handle,
            &[],
            |catalog| Value::Array(catalog.snapshot()),
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Serializes one effect metadata object, or an empty object when the ID is absent.
///
/// # Safety
///
/// The handle must be live, `id` readable, and output ranges valid and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_effect_catalog_state_find_json(
    handle: *const AviQtlEffectCatalogState,
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
        catalog_json(
            handle,
            &[(id, id_length)],
            |catalog| Value::Object(catalog.find(id_value)),
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Removes every effect whose ID appears in the input JSON array.
///
/// # Safety
///
/// The handle must be live and `input` must contain a readable JSON array of strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_effect_catalog_state_remove_ids_json(
    handle: *mut AviQtlEffectCatalogState,
    input: *const u8,
    input_length: usize,
) -> u32 {
    if handle.is_null() || !slice_is_valid(input, input_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The input range was validated above.
    let Some(ids) = serde_json::from_slice::<Value>(unsafe { input_bytes(input, input_length) })
        .ok()
        .and_then(|value| value.as_array().cloned())
    else {
        return STATUS_INVALID_JSON;
    };
    if ids.iter().any(|id| !id.is_string()) {
        return STATUS_INVALID_ARGUMENT;
    }
    with_catalog_mut(handle, |catalog| catalog.remove_ids(&ids))
        .map(|()| STATUS_OK)
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_schema_accepts_effects_and_normalizes_objects() {
        let input = json!({
            "id": "effect.test",
            "name": "Test",
            "qml": "Test.qml",
            "version": "1.2.3",
            "kind": "effect",
            "categories": ["Color", 5],
            "params": {"amount": 1},
            "ui": {"controls": []}
        });
        let normalized =
            normalize_metadata(&serde_json::to_vec(&input).expect("serialize metadata fixture"))
                .expect("valid metadata");
        assert_eq!(normalized.get("categories"), Some(&json!(["Color"])));
        assert_eq!(normalized.get("params"), Some(&json!({"amount": 1})));
    }

    #[test]
    fn metadata_schema_rejects_invalid_versions_kinds_and_controls() {
        for invalid in [
            json!({"id":"x","name":"x","qml":"x.qml","version":"1.0","kind":"effect","categories":["x"],"ui":{"controls":[]}}),
            json!({"id":"x","name":"x","qml":"x.qml","version":"1.0.0","kind":"filter","categories":["x"],"ui":{"controls":[]}}),
            json!({"id":"x","name":"x","qml":"x.qml","version":"1.0.0","kind":"effect","categories":["x"],"ui":{}}),
        ] {
            assert!(
                normalize_metadata(
                    &serde_json::to_vec(&invalid).expect("serialize metadata fixture")
                )
                .is_none()
            );
        }
    }

    #[test]
    fn catalog_state_owns_replacement_order_lookup_and_removal() {
        let mut catalog = EffectCatalog::default();
        assert!(
            catalog.register(
                json!({"id":"effect.alpha","name":"Alpha","version":"1.0.0"})
                    .as_object()
                    .cloned()
                    .unwrap()
            )
        );
        assert!(
            catalog.register(
                json!({"id":"effect.beta","name":"Beta","version":"1.0.0"})
                    .as_object()
                    .cloned()
                    .unwrap()
            )
        );
        assert!(
            catalog.register(
                json!({"id":"effect.alpha","name":"Alpha 2","version":"2.0.0"})
                    .as_object()
                    .cloned()
                    .unwrap()
            )
        );
        assert!(!catalog.register(Map::new()));

        let snapshot = catalog.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0]["id"], "effect.alpha");
        assert_eq!(snapshot[0]["name"], "Alpha 2");
        assert_eq!(snapshot[1]["id"], "effect.beta");
        assert_eq!(catalog.find("effect.beta")["name"], "Beta");
        assert!(catalog.find("missing").is_empty());

        catalog.remove_ids(&[Value::String("effect.alpha".to_owned())]);
        assert!(catalog.find("effect.alpha").is_empty());
        assert_eq!(catalog.snapshot().len(), 1);
        assert_eq!(catalog.snapshot()[0]["id"], "effect.beta");
    }
}
