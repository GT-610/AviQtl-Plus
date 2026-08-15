use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use serde_json::{Map, Value};

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
}
