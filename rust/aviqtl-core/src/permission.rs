use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use crate::policy::{PERMISSION_NAMES, permission_from_name};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Default)]
struct PermissionState {
    plugins: BTreeMap<String, u64>,
}

impl PermissionState {
    fn from_json(input: &[u8]) -> Option<Self> {
        let root = serde_json::from_slice::<Value>(input).ok()?;
        let object = root.as_object()?;
        let mut plugins = BTreeMap::new();
        for (plugin_id, permissions) in object {
            let mut mask = 0_u64;
            if let Some(names) = permissions.as_array() {
                for name in names.iter().filter_map(Value::as_str) {
                    let permission = permission_from_name(name);
                    if permission >= 0 {
                        mask |= 1_u64 << permission;
                    }
                }
            }
            plugins.insert(plugin_id.clone(), mask);
        }
        Some(Self { plugins })
    }

    fn to_json(&self) -> Value {
        let permissions = self
            .plugins
            .iter()
            .map(|(plugin_id, mask)| {
                let names = PERMISSION_NAMES
                    .iter()
                    .enumerate()
                    .filter(|(permission, _)| mask & (1_u64 << permission) != 0)
                    .map(|(_, name)| Value::String((*name).to_owned()))
                    .collect();
                (plugin_id.clone(), Value::Array(names))
            })
            .collect::<Map<_, _>>();
        Value::Object(permissions)
    }

    fn has(&self, plugin_id: &str, permission: i32) -> bool {
        permission_is_valid(permission)
            && self
                .plugins
                .get(plugin_id)
                .is_some_and(|mask| mask & (1_u64 << permission) != 0)
    }

    fn grant(&mut self, plugin_id: &str, permission: i32) -> bool {
        if !permission_is_valid(permission) {
            return false;
        }
        *self.plugins.entry(plugin_id.to_owned()).or_default() |= 1_u64 << permission;
        true
    }

    fn revoke(&mut self, plugin_id: &str, permission: i32) -> Option<bool> {
        if !permission_is_valid(permission) {
            return None;
        }
        let Some(mask) = self.plugins.get_mut(plugin_id) else {
            return Some(false);
        };
        *mask &= !(1_u64 << permission);
        if *mask == 0 {
            self.plugins.remove(plugin_id);
        }
        Some(true)
    }

    fn grant_all(&mut self, plugin_id: &str) {
        let all = (1_u64 << PERMISSION_NAMES.len()) - 1;
        self.plugins.insert(plugin_id.to_owned(), all);
    }

    fn revoke_all(&mut self, plugin_id: &str) {
        self.plugins.remove(plugin_id);
    }

    fn mask(&self, plugin_id: &str) -> u64 {
        self.plugins.get(plugin_id).copied().unwrap_or(0)
    }
}

pub struct AviQtlPermissionState {
    state: Mutex<PermissionState>,
}

fn permission_is_valid(permission: i32) -> bool {
    usize::try_from(permission).is_ok_and(|permission| permission < PERMISSION_NAMES.len())
}

unsafe fn input_bytes<'a>(input: *const u8, input_length: usize) -> &'a [u8] {
    if input_length == 0 {
        &[]
    } else {
        // SAFETY: The caller validates the readable range before invoking this helper.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    }
}

unsafe fn plugin_id<'a>(input: *const u8, input_length: usize) -> Option<&'a str> {
    if !slice_is_valid(input, input_length) {
        return None;
    }
    // SAFETY: The byte range was validated above and remains owned by the caller.
    std::str::from_utf8(unsafe { input_bytes(input, input_length) }).ok()
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

unsafe fn write_json(
    value: &PermissionState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let Ok(json) = serde_json::to_vec(&value.to_json()) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The caller validated the output-length pointer and de-overlapped it from output.
    unsafe { output_length.write(json.len()) };
    if output_capacity < json.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !json.is_empty() {
        // SAFETY: The writable output range was validated for the full capacity.
        unsafe { std::ptr::copy_nonoverlapping(json.as_ptr(), output, json.len()) };
    }
    STATUS_OK
}

fn with_state<T>(
    handle: *const AviQtlPermissionState,
    operation: impl FnOnce(&PermissionState) -> T,
) -> Option<T> {
    // SAFETY: Non-null handles are required to originate from `aviqtl_permission_state_create`.
    let state = unsafe { handle.as_ref() }?;
    let guard = state.state.lock().ok()?;
    Some(operation(&guard))
}

fn with_state_mut<T>(
    handle: *mut AviQtlPermissionState,
    operation: impl FnOnce(&mut PermissionState) -> T,
) -> Option<T> {
    // SAFETY: Non-null handles are required to originate from `aviqtl_permission_state_create`.
    let state = unsafe { handle.as_ref() }?;
    let mut guard = state.state.lock().ok()?;
    Some(operation(&mut guard))
}

/// Creates Rust-owned plugin permission state from a JSON object.
///
/// # Safety
///
/// The input range must be readable UTF-8 JSON. `output_handle` must point to writable storage
/// for one handle and must not overlap the input range. A returned handle must be destroyed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_create(
    input: *const u8,
    input_length: usize,
    output_handle: *mut *mut AviQtlPermissionState,
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
    let Some(state) = PermissionState::from_json(unsafe { input_bytes(input, input_length) })
    else {
        return STATUS_INVALID_JSON;
    };
    let handle = Box::into_raw(Box::new(AviQtlPermissionState {
        state: Mutex::new(state),
    }));
    // SAFETY: The output-handle range was validated and does not overlap the input.
    unsafe { output_handle.write(handle) };
    STATUS_OK
}

/// Destroys Rust-owned plugin permission state. A null handle is accepted.
///
/// # Safety
///
/// A non-null handle must have been returned by `aviqtl_permission_state_create` exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_destroy(handle: *mut AviQtlPermissionState) {
    if !handle.is_null() {
        // SAFETY: The caller guarantees unique ownership of a live handle.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Atomically replaces plugin permission state from a JSON object.
///
/// # Safety
///
/// The handle must be live and the input range must remain readable for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_reset(
    handle: *mut AviQtlPermissionState,
    input: *const u8,
    input_length: usize,
) -> u32 {
    if handle.is_null() || !slice_is_valid(input, input_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The input range was validated above.
    let Some(replacement) = PermissionState::from_json(unsafe { input_bytes(input, input_length) })
    else {
        return STATUS_INVALID_JSON;
    };
    with_state_mut(handle, |state| *state = replacement)
        .map(|()| STATUS_OK)
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Serializes the current permission state to canonical JSON.
///
/// # Safety
///
/// The handle must be live. Output ranges must be valid, writable, and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_snapshot_json(
    handle: *const AviQtlPermissionState,
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
    with_state(handle, |state| {
        // SAFETY: Output ranges were validated and checked for overlap above.
        unsafe { write_json(state, output, output_capacity, output_length) }
    })
    .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Returns whether one plugin owns one permission.
///
/// # Safety
///
/// The handle must be live and the plugin ID range must contain valid UTF-8 for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_has(
    handle: *const AviQtlPermissionState,
    plugin: *const u8,
    plugin_length: usize,
    permission: i32,
) -> u32 {
    // SAFETY: The helper validates the plugin byte range before decoding UTF-8.
    let Some(plugin) = (unsafe { plugin_id(plugin, plugin_length) }) else {
        return 0;
    };
    with_state(handle, |state| u32::from(state.has(plugin, permission))).unwrap_or(0)
}

/// Grants one permission to a plugin.
///
/// # Safety
///
/// The handle must be live and the plugin ID range must contain valid UTF-8 for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_grant(
    handle: *mut AviQtlPermissionState,
    plugin: *const u8,
    plugin_length: usize,
    permission: i32,
) -> u32 {
    // SAFETY: The helper validates the plugin byte range before decoding UTF-8.
    let Some(plugin) = (unsafe { plugin_id(plugin, plugin_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    match with_state_mut(handle, |state| state.grant(plugin, permission)) {
        Some(true) => STATUS_OK,
        _ => STATUS_INVALID_ARGUMENT,
    }
}

/// Revokes one permission and reports whether the plugin existed before the operation.
///
/// # Safety
///
/// The handle and plugin ID must be valid. `plugin_existed` must be writable and may not overlap
/// the plugin ID range. It is written only after all arguments have been validated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_revoke(
    handle: *mut AviQtlPermissionState,
    plugin: *const u8,
    plugin_length: usize,
    permission: i32,
    plugin_existed: *mut u32,
) -> u32 {
    if !slice_is_valid(plugin_existed, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    match ranges_overlap(plugin, plugin_length, plugin_existed, 1) {
        Some(true) => return STATUS_OVERLAPPING_BUFFERS,
        Some(false) => {}
        None => return STATUS_INVALID_ARGUMENT,
    }
    // SAFETY: The helper validates the plugin byte range before decoding UTF-8.
    let Some(plugin) = (unsafe { plugin_id(plugin, plugin_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(existed) = with_state_mut(handle, |state| state.revoke(plugin, permission)) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(existed) = existed else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The output pointer was validated and checked against the plugin input range.
    unsafe { plugin_existed.write(u32::from(existed)) };
    STATUS_OK
}

/// Grants every known permission to one plugin.
///
/// # Safety
///
/// The handle must be live and the plugin ID range must contain valid UTF-8 for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_grant_all(
    handle: *mut AviQtlPermissionState,
    plugin: *const u8,
    plugin_length: usize,
) -> u32 {
    // SAFETY: The helper validates the plugin byte range before decoding UTF-8.
    let Some(plugin) = (unsafe { plugin_id(plugin, plugin_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    with_state_mut(handle, |state| state.grant_all(plugin))
        .map(|()| STATUS_OK)
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Removes all permissions and state for one plugin.
///
/// # Safety
///
/// The handle must be live and the plugin ID range must contain valid UTF-8 for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_revoke_all(
    handle: *mut AviQtlPermissionState,
    plugin: *const u8,
    plugin_length: usize,
) -> u32 {
    // SAFETY: The helper validates the plugin byte range before decoding UTF-8.
    let Some(plugin) = (unsafe { plugin_id(plugin, plugin_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    with_state_mut(handle, |state| state.revoke_all(plugin))
        .map(|()| STATUS_OK)
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Returns the canonical permission bit mask for one plugin, or zero for invalid input.
///
/// # Safety
///
/// The handle must be live and the plugin ID range must contain valid UTF-8 for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_mask(
    handle: *const AviQtlPermissionState,
    plugin: *const u8,
    plugin_length: usize,
) -> u64 {
    // SAFETY: The helper validates the plugin byte range before decoding UTF-8.
    let Some(plugin) = (unsafe { plugin_id(plugin, plugin_length) }) else {
        return 0;
    };
    with_state(handle, |state| state.mask(plugin)).unwrap_or(0)
}

/// Returns whether one plugin owns at least one known permission.
///
/// # Safety
///
/// The handle must be live and the plugin ID range must contain valid UTF-8 for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_permission_state_is_authorized(
    handle: *const AviQtlPermissionState,
    plugin: *const u8,
    plugin_length: usize,
) -> u32 {
    u32::from(unsafe { aviqtl_permission_state_mask(handle, plugin, plugin_length) } != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_normalizes_unknown_names_and_preserves_empty_plugin_entries() {
        let state = PermissionState::from_json(
            br#"{
                "plugin.b": ["clip.read", "unknown", "clip.read"],
                "plugin.a": "invalid"
            }"#,
        )
        .expect("valid permission document");
        assert!(state.has("plugin.b", 1));
        assert!(!state.has("plugin.b", 2));
        assert_eq!(state.mask("plugin.a"), 0);
        assert_eq!(
            state.to_json(),
            serde_json::json!({
                "plugin.a": [],
                "plugin.b": ["clip.read"]
            })
        );
    }

    #[test]
    fn mutations_are_idempotent_and_remove_empty_plugin_state() {
        let mut state = PermissionState::default();
        assert!(state.grant("plugin", 0));
        assert!(state.grant("plugin", 0));
        assert!(state.has("plugin", 0));
        assert_eq!(state.revoke("plugin", 1), Some(true));
        assert_eq!(state.mask("plugin"), 1);
        assert_eq!(state.revoke("plugin", 0), Some(true));
        assert!(!state.plugins.contains_key("plugin"));
        assert_eq!(state.revoke("plugin", 0), Some(false));
        assert_eq!(state.revoke("plugin", -1), None);

        state.grant_all("plugin");
        assert_eq!(
            state.mask("plugin").count_ones() as usize,
            PERMISSION_NAMES.len()
        );
        state.revoke_all("plugin");
        assert_eq!(state.mask("plugin"), 0);
    }

    #[test]
    fn ffi_reset_is_atomic_and_snapshot_uses_two_phase_output() {
        let initial = br#"{"plugin":["transport.control"]}"#;
        let mut handle = std::ptr::null_mut();
        // SAFETY: All byte and handle-output ranges are valid and disjoint.
        assert_eq!(
            unsafe { aviqtl_permission_state_create(initial.as_ptr(), initial.len(), &mut handle) },
            STATUS_OK
        );
        assert!(!handle.is_null());

        let invalid = b"not-json";
        // SAFETY: The live handle and readable input range satisfy the boundary contract.
        assert_eq!(
            unsafe { aviqtl_permission_state_reset(handle, invalid.as_ptr(), invalid.len()) },
            STATUS_INVALID_JSON
        );
        // SAFETY: The live handle and plugin byte range are valid.
        assert_eq!(
            unsafe { aviqtl_permission_state_has(handle, b"plugin".as_ptr(), 6, 0) },
            1
        );

        let mut required = 0;
        // SAFETY: A null zero-capacity output is valid for the size query.
        assert_eq!(
            unsafe {
                aviqtl_permission_state_snapshot_json(
                    handle,
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut output = vec![0_u8; required];
        let mut written = 0;
        // SAFETY: The output and length ranges are valid, writable, and disjoint.
        assert_eq!(
            unsafe {
                aviqtl_permission_state_snapshot_json(
                    handle,
                    output.as_mut_ptr(),
                    output.len(),
                    &mut written,
                )
            },
            STATUS_OK
        );
        assert_eq!(written, output.len());
        assert_eq!(
            serde_json::from_slice::<Value>(&output).expect("snapshot JSON"),
            serde_json::json!({"plugin": ["transport.control"]})
        );

        // SAFETY: The handle was returned by create and has not yet been destroyed.
        unsafe { aviqtl_permission_state_destroy(handle) };
    }

    #[test]
    fn ffi_revoke_reports_legacy_plugin_existence_semantics() {
        let initial = br#"{"plugin":["transport.control"]}"#;
        let mut handle = std::ptr::null_mut();
        // SAFETY: All byte and handle-output ranges are valid and disjoint.
        assert_eq!(
            unsafe { aviqtl_permission_state_create(initial.as_ptr(), initial.len(), &mut handle) },
            STATUS_OK
        );
        let mut existed = 0;
        // Revoking an absent permission still reports that the plugin entry existed.
        // SAFETY: The live handle, UTF-8 plugin range, and output are valid and disjoint.
        assert_eq!(
            unsafe {
                aviqtl_permission_state_revoke(handle, b"plugin".as_ptr(), 6, 1, &mut existed)
            },
            STATUS_OK
        );
        assert_eq!(existed, 1);
        assert_eq!(
            unsafe {
                aviqtl_permission_state_revoke(handle, b"missing".as_ptr(), 7, 1, &mut existed)
            },
            STATUS_OK
        );
        assert_eq!(existed, 0);
        // SAFETY: The handle was returned by create and has not yet been destroyed.
        unsafe { aviqtl_permission_state_destroy(handle) };
    }
}
