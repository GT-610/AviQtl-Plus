use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid, utf8,
};
use crate::policy::{valid_recovery_id, valid_recovery_snapshot_name};
use serde_json::{Map, Value, json};

fn text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn number(bytes: &[u8], start: usize, length: usize) -> Option<u32> {
    let digits = bytes.get(start..start.checked_add(length)?)?;
    if digits.iter().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    digits.iter().try_fold(0_u32, |value, digit| {
        value.checked_mul(10)?.checked_add(u32::from(digit - b'0'))
    })
}

fn leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn valid_iso_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 19
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let Some(year) = number(bytes, 0, 4) else {
        return false;
    };
    let Some(month) = number(bytes, 5, 2) else {
        return false;
    };
    let Some(day) = number(bytes, 8, 2) else {
        return false;
    };
    let Some(hour) = number(bytes, 11, 2) else {
        return false;
    };
    let Some(minute) = number(bytes, 14, 2) else {
        return false;
    };
    let Some(second) = number(bytes, 17, 2) else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year(year) => 29,
        2 => 28,
        _ => return false,
    };
    if year == 0 || day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return false;
    }

    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    if index == bytes.len() {
        return true;
    }
    if bytes.get(index) == Some(&b'Z') {
        return index + 1 == bytes.len();
    }
    if !matches!(bytes.get(index), Some(b'+') | Some(b'-')) || index + 6 != bytes.len() {
        return false;
    }
    bytes.get(index + 3) == Some(&b':')
        && number(bytes, index + 1, 2).is_some_and(|hour| hour <= 23)
        && number(bytes, index + 4, 2).is_some_and(|minute| minute <= 59)
}

fn snapshot_file_name(id: &str, metadata: &Map<String, Value>) -> String {
    let file_name = metadata
        .get("snapshotFile")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{id}.aviqtl"));
    if valid_recovery_snapshot_name(id, &file_name) {
        file_name
    } else {
        String::new()
    }
}

fn inspect_metadata(id: &str, metadata: &Map<String, Value>) -> Map<String, Value> {
    let metadata_id = text(metadata.get("id"));
    let saved_at = text(metadata.get("savedAt"));
    let snapshot_file = snapshot_file_name(id, metadata);
    let status = if !valid_recovery_id(id) {
        "invalid_id"
    } else if metadata_id != id {
        "mismatched_id"
    } else if snapshot_file.is_empty() {
        "invalid_snapshot"
    } else if !valid_iso_timestamp(&saved_at) {
        "invalid_timestamp"
    } else {
        "ok"
    };
    json!({
        "id": id,
        "originalProjectUrl": text(metadata.get("originalProjectUrl")),
        "displayName": text(metadata.get("displayName")),
        "savedAt": saved_at,
        "snapshotFile": snapshot_file,
        "status": status,
    })
    .as_object()
    .cloned()
    .expect("inspection is an object")
}

fn metadata_document(
    id: &str,
    original_project_url: &str,
    display_name: &str,
    saved_at: &str,
    snapshot_file: &str,
) -> Option<Map<String, Value>> {
    if !valid_recovery_id(id)
        || !valid_recovery_snapshot_name(id, snapshot_file)
        || !valid_iso_timestamp(saved_at)
    {
        return None;
    }
    json!({
        "id": id,
        "originalProjectUrl": original_project_url,
        "displayName": display_name,
        "savedAt": saved_at,
        "snapshotFile": snapshot_file,
    })
    .as_object()
    .cloned()
}

fn recovery_id_from_snapshot(file_name: &str) -> String {
    let Some(id) = file_name.get(..36) else {
        return String::new();
    };
    if valid_recovery_snapshot_name(id, file_name) {
        id.to_owned()
    } else {
        String::new()
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

unsafe fn write_bytes(
    bytes: &[u8],
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: The caller validates and de-overlaps the output-length range.
    unsafe { output_length.write(bytes.len()) };
    if output_capacity < bytes.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !bytes.is_empty() {
        // SAFETY: The caller validates the output range and capacity.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..bytes.len()].copy_from_slice(bytes);
    }
    STATUS_OK
}

unsafe fn write_json(
    value: &impl serde::Serialize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let Ok(json) = serde_json::to_vec(value) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The caller validates all output ranges.
    unsafe { write_bytes(&json, output, output_capacity, output_length) }
}

/// Parses and validates one recovery metadata document against its metadata-file ID.
///
/// # Safety
///
/// Input ranges must be readable and output ranges writable, valid, and pairwise disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_recovery_metadata_inspect_json(
    id: *const u8,
    id_length: usize,
    metadata: *const u8,
    metadata_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let Some(id_value) = (unsafe { utf8(id, id_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if !slice_is_valid(metadata, metadata_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    if let Err(status) = output_ranges_valid(
        &[(id, id_length), (metadata, metadata_length)],
        output,
        output_capacity,
        output_length,
    ) {
        return status;
    }
    let metadata_bytes = if metadata_length == 0 {
        &[]
    } else {
        // SAFETY: The metadata range was validated above.
        unsafe { std::slice::from_raw_parts(metadata, metadata_length) }
    };
    let Some(metadata) = serde_json::from_slice::<Value>(metadata_bytes)
        .ok()
        .and_then(|value| value.as_object().cloned())
    else {
        return STATUS_INVALID_JSON;
    };
    let inspection = inspect_metadata(id_value, &metadata);
    // SAFETY: Output ranges were validated and checked against both inputs.
    unsafe { write_json(&inspection, output, output_capacity, output_length) }
}

/// Builds one validated recovery metadata JSON document.
///
/// # Safety
///
/// Input ranges must be readable and output ranges writable, valid, and pairwise disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_recovery_metadata_build_json(
    id: *const u8,
    id_length: usize,
    original_project_url: *const u8,
    original_project_url_length: usize,
    display_name: *const u8,
    display_name_length: usize,
    saved_at: *const u8,
    saved_at_length: usize,
    snapshot_file: *const u8,
    snapshot_file_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let inputs = [
        (id, id_length),
        (original_project_url, original_project_url_length),
        (display_name, display_name_length),
        (saved_at, saved_at_length),
        (snapshot_file, snapshot_file_length),
    ];
    if let Err(status) = output_ranges_valid(&inputs, output, output_capacity, output_length) {
        return status;
    }
    let Some(id) = (unsafe { utf8(id, id_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(original_project_url) =
        (unsafe { utf8(original_project_url, original_project_url_length) })
    else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(display_name) = (unsafe { utf8(display_name, display_name_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(saved_at) = (unsafe { utf8(saved_at, saved_at_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(snapshot_file) = (unsafe { utf8(snapshot_file, snapshot_file_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(metadata) = metadata_document(
        id,
        original_project_url,
        display_name,
        saved_at,
        snapshot_file,
    ) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: Output ranges were validated and checked against every input.
    unsafe { write_json(&metadata, output, output_capacity, output_length) }
}

/// Extracts the owning recovery ID from a canonical snapshot file name.
///
/// # Safety
///
/// The input range must be readable and output ranges writable, valid, and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_recovery_snapshot_id(
    file_name: *const u8,
    file_name_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let Some(file_name_value) = (unsafe { utf8(file_name, file_name_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if let Err(status) = output_ranges_valid(
        &[(file_name, file_name_length)],
        output,
        output_capacity,
        output_length,
    ) {
        return status;
    }
    let id = recovery_id_from_snapshot(file_name_value);
    // SAFETY: Output ranges were validated and checked against the input.
    unsafe { write_bytes(id.as_bytes(), output, output_capacity, output_length) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_validate_calendar_fraction_and_offsets() {
        assert!(valid_iso_timestamp("2026-09-04T12:30:45.123Z"));
        assert!(valid_iso_timestamp("2024-02-29T12:30:45+08:00"));
        assert!(valid_iso_timestamp("2026-09-04T12:30:45"));
        assert!(!valid_iso_timestamp("2025-02-29T12:30:45Z"));
        assert!(!valid_iso_timestamp("2026-09-04T25:30:45Z"));
        assert!(!valid_iso_timestamp("2026-09-04T12:30:45."));
    }

    #[test]
    fn metadata_rules_build_inspect_and_preserve_legacy_snapshot_fallback() {
        let id = "01234567-89ab-cdef-0123-456789abcdef";
        let generation = "fedcba98-7654-3210-fedc-ba9876543210";
        let snapshot = format!("{id}-{generation}.aviqtl");
        let metadata = metadata_document(
            id,
            "file:///project.aviqtl",
            "Project",
            "2026-09-04T12:30:45.123Z",
            &snapshot,
        )
        .expect("valid metadata");
        let inspection = inspect_metadata(id, &metadata);
        assert_eq!(text(inspection.get("status")), "ok");
        assert_eq!(text(inspection.get("snapshotFile")), snapshot);

        let legacy = json!({
            "id": id,
            "displayName": "Legacy",
            "savedAt": "2026-09-04T12:30:45Z"
        });
        let inspection = inspect_metadata(id, legacy.as_object().expect("fixture"));
        assert_eq!(text(inspection.get("status")), "ok");
        assert_eq!(text(inspection.get("snapshotFile")), format!("{id}.aviqtl"));

        let mismatch = inspect_metadata(
            id,
            json!({
                "id": generation,
                "savedAt": "2026-09-04T12:30:45Z"
            })
            .as_object()
            .expect("fixture"),
        );
        assert_eq!(text(mismatch.get("status")), "mismatched_id");
    }

    #[test]
    fn snapshot_name_extraction_rejects_unowned_and_malformed_files() {
        let id = "01234567-89ab-cdef-0123-456789abcdef";
        let generation = "fedcba98-7654-3210-fedc-ba9876543210";
        assert_eq!(recovery_id_from_snapshot(&format!("{id}.aviqtl")), id);
        assert_eq!(
            recovery_id_from_snapshot(&format!("{id}-{generation}.aviqtl")),
            id
        );
        assert!(recovery_id_from_snapshot(&format!("../{id}.aviqtl")).is_empty());
        assert!(recovery_id_from_snapshot("not-a-recovery.aviqtl").is_empty());
    }
}
