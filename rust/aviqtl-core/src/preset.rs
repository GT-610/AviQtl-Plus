use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid, utf8,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

const PRESET_VERSION: i32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PresetDocument {
    version: i32,
    #[serde(rename = "effectId")]
    effect_id: String,
    name: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    params: Map<String, Value>,
    #[serde(default)]
    keyframes: Map<String, Value>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && !value.contains("..")
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('\0')
}

fn parse_object(input: &[u8]) -> Option<Map<String, Value>> {
    serde_json::from_slice::<Value>(input)
        .ok()?
        .as_object()
        .cloned()
}

fn build_document(
    effect_id: &str,
    name: &str,
    enabled: bool,
    params: Map<String, Value>,
    keyframes: Map<String, Value>,
) -> Option<PresetDocument> {
    if !safe_name(effect_id) || !safe_name(name) {
        return None;
    }
    Some(PresetDocument {
        version: PRESET_VERSION,
        effect_id: effect_id.to_owned(),
        name: name.to_owned(),
        enabled,
        params,
        keyframes,
        extra: BTreeMap::new(),
    })
}

fn normalize_document(input: &[u8], effect_id: &str, name: &str) -> Option<PresetDocument> {
    if !safe_name(effect_id) || !safe_name(name) {
        return None;
    }
    let document: PresetDocument = serde_json::from_slice(input).ok()?;
    if document.version != PRESET_VERSION
        || document.effect_id != effect_id
        || document.name != name
        || !safe_name(&document.effect_id)
        || !safe_name(&document.name)
    {
        return None;
    }
    Some(document)
}

unsafe fn bytes<'a>(input: *const u8, input_length: usize) -> &'a [u8] {
    if input_length == 0 {
        &[]
    } else {
        // SAFETY: Callers validate the full input range before invoking this helper.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    }
}

fn validate_transform_ranges(
    inputs: &[(*const u8, usize)],
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> Result<(), u32> {
    if inputs
        .iter()
        .any(|(input, length)| !slice_is_valid(*input, *length))
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
    {
        return Err(STATUS_INVALID_ARGUMENT);
    }
    for &(input, length) in inputs {
        let overlaps = [
            ranges_overlap(input, length, output, output_capacity),
            ranges_overlap(input, length, output_length, 1),
        ];
        if overlaps.iter().any(Option::is_none) {
            return Err(STATUS_INVALID_ARGUMENT);
        }
        if overlaps.into_iter().flatten().any(|overlap| overlap) {
            return Err(STATUS_OVERLAPPING_BUFFERS);
        }
    }
    match ranges_overlap(output, output_capacity, output_length, 1) {
        None => Err(STATUS_INVALID_ARGUMENT),
        Some(true) => Err(STATUS_OVERLAPPING_BUFFERS),
        Some(false) => Ok(()),
    }
}

unsafe fn write_json(
    document: &PresetDocument,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let Ok(json) = serde_json::to_vec(document) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The caller validates and de-overlaps the output-length pointer.
    unsafe { output_length.write(json.len()) };
    if output_capacity < json.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !json.is_empty() {
        // SAFETY: The caller validates the writable output range and its capacity.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..json.len()].copy_from_slice(&json);
    }
    STATUS_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_preset_name_is_safe(value: *const u8, value_length: usize) -> u32 {
    // SAFETY: The helper validates the pointer/length pair before borrowing it.
    u32::from(unsafe { utf8(value, value_length) }.is_some_and(safe_name))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_preset_build_json(
    effect_id: *const u8,
    effect_id_length: usize,
    name: *const u8,
    name_length: usize,
    enabled: u32,
    params: *const u8,
    params_length: usize,
    keyframes: *const u8,
    keyframes_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let inputs = [
        (effect_id, effect_id_length),
        (name, name_length),
        (params, params_length),
        (keyframes, keyframes_length),
    ];
    if let Err(status) = validate_transform_ranges(&inputs, output, output_capacity, output_length)
    {
        return status;
    }
    // SAFETY: The input ranges were validated above and remain alive for this call.
    let Some(effect_id) = (unsafe { utf8(effect_id, effect_id_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The input ranges were validated above and remain alive for this call.
    let Some(name) = (unsafe { utf8(name, name_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: JSON input ranges were validated above.
    let params = unsafe { bytes(params, params_length) };
    // SAFETY: JSON input ranges were validated above.
    let keyframes = unsafe { bytes(keyframes, keyframes_length) };
    let Some(params) = parse_object(params) else {
        return STATUS_INVALID_JSON;
    };
    let Some(keyframes) = parse_object(keyframes) else {
        return STATUS_INVALID_JSON;
    };
    let Some(document) = build_document(effect_id, name, enabled != 0, params, keyframes) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: Output ranges were validated and checked for overlap above.
    unsafe { write_json(&document, output, output_capacity, output_length) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_preset_normalize_json(
    effect_id: *const u8,
    effect_id_length: usize,
    name: *const u8,
    name_length: usize,
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let inputs = [
        (effect_id, effect_id_length),
        (name, name_length),
        (input, input_length),
    ];
    if let Err(status) = validate_transform_ranges(&inputs, output, output_capacity, output_length)
    {
        return status;
    }
    // SAFETY: The input ranges were validated above and remain alive for this call.
    let Some(effect_id) = (unsafe { utf8(effect_id, effect_id_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The input ranges were validated above and remain alive for this call.
    let Some(name) = (unsafe { utf8(name, name_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The document input range was validated above.
    let input = unsafe { bytes(input, input_length) };
    let Some(document) = normalize_document(input, effect_id, name) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: Output ranges were validated and checked for overlap above.
    unsafe { write_json(&document, output, output_capacity, output_length) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_reject_hidden_and_traversal_components() {
        assert!(safe_name("Warm Look"));
        assert!(!safe_name(""));
        assert!(!safe_name(".hidden"));
        assert!(!safe_name("../escape"));
        assert!(!safe_name("nested/name"));
        assert!(!safe_name("nested\\name"));
    }

    #[test]
    fn document_build_and_normalization_enforce_identity_and_schema() {
        let document = build_document(
            "effect.test",
            "Warm Look",
            true,
            json!({"amount": 2}).as_object().cloned().expect("fixture"),
            Map::new(),
        )
        .expect("valid document");
        let json = serde_json::to_vec(&document).expect("serialize fixture");
        assert_eq!(
            normalize_document(&json, "effect.test", "Warm Look"),
            Some(document)
        );
        assert!(normalize_document(&json, "effect.other", "Warm Look").is_none());

        let manually_edited = json!({
            "version": 1,
            "effectId": "effect.test",
            "name": "Warm Look"
        });
        let normalized = normalize_document(
            &serde_json::to_vec(&manually_edited).expect("serialize fixture"),
            "effect.test",
            "Warm Look",
        )
        .expect("omitted fields use defaults");
        assert!(!normalized.enabled);
        assert!(normalized.params.is_empty());
        assert!(normalized.keyframes.is_empty());

        let unsupported = json!({
            "version": 2,
            "effectId": "effect.test",
            "name": "Warm Look",
            "enabled": true,
            "params": {},
            "keyframes": {}
        });
        assert!(
            normalize_document(
                &serde_json::to_vec(&unsupported).expect("serialize fixture"),
                "effect.test",
                "Warm Look"
            )
            .is_none()
        );
    }
}
