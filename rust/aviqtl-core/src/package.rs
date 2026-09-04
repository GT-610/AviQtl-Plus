use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use crate::policy::valid_package_id;
use semver::Version;
use serde_json::{Map, Value, json};
use std::sync::Mutex;

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn integer(value: Option<&Value>, fallback: i32) -> i32 {
    value
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(fallback)
}

fn boolean(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
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

fn supported_package_type(value: &str) -> bool {
    matches!(value, "application" | "mod" | "effect" | "object")
}

fn numeric_version_part(value: &str) -> Option<i32> {
    value.trim().parse::<i32>().ok()
}

pub(crate) fn compare_versions(left: &str, right: &str) -> i32 {
    if left == right {
        return 0;
    }
    let left = left.strip_prefix('v').unwrap_or(left);
    let right = right.strip_prefix('v').unwrap_or(right);
    if let (Ok(left), Ok(right)) = (Version::parse(left), Version::parse(right)) {
        return match left.cmp_precedence(&right) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
    }
    let left_parts: Vec<_> = left.split('.').collect();
    let right_parts: Vec<_> = right.split('.').collect();
    for (left, right) in left_parts.iter().zip(&right_parts) {
        let ordering = match (numeric_version_part(left), numeric_version_part(right)) {
            (Some(left), Some(right)) => left.cmp(&right),
            _ => left.cmp(right),
        };
        if !ordering.is_eq() {
            return if ordering.is_lt() { -1 } else { 1 };
        };
    }
    match left_parts.len().cmp(&right_parts.len()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn localized(field: Option<&Value>, fallback: &str, language: &str) -> String {
    if let Some(map) = field.and_then(Value::as_object) {
        if let Some(value) = map.get(language) {
            return text(Some(value));
        }
        if let Some(value) = map.get("en") {
            return text(Some(value));
        }
        return map
            .values()
            .next()
            .map(|value| text(Some(value)))
            .unwrap_or_else(|| fallback.to_owned());
    }
    let value = text(field);
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

fn copy_value(source: &Map<String, Value>, key: &str) -> Value {
    source.get(key).cloned().unwrap_or(Value::Null)
}

fn installed_version(installed: &Map<String, Value>, id: &str, app_version: &str) -> String {
    if id == "org.aviqtl.app" {
        return app_version.to_owned();
    }
    installed
        .get(id)
        .and_then(Value::as_object)
        .map(|entry| text(entry.get("version")))
        .unwrap_or_default()
}

fn resolved_repository_priority(repositories: &[Map<String, Value>], url: &str) -> i32 {
    repositories
        .iter()
        .find(|repository| text(repository.get("url")) == url)
        .map(|repository| integer(repository.get("priority"), 10))
        .unwrap_or(i32::MAX)
}

fn merge_catalog_package(
    mut catalog: Vec<Map<String, Value>>,
    package: &Map<String, Value>,
    repository: &Map<String, Value>,
    repositories: &[Map<String, Value>],
    installed: &Map<String, Value>,
    language: &str,
    app_version: &str,
) -> Vec<Map<String, Value>> {
    let id = text(package.get("id"));
    let package_type = text(package.get("type"));
    if !valid_package_id(&id) || !supported_package_type(&package_type) {
        return catalog;
    }
    let repository_url = text(repository.get("url"));
    let new_repository_priority = resolved_repository_priority(repositories, &repository_url);
    let version = text(package.get("version"));
    let mut sources = package
        .get("_sources")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    sources.insert(repository_url.clone(), Value::String(version.clone()));

    let mut entry = Map::new();
    entry.insert("id".to_owned(), Value::String(id.clone()));
    entry.insert("type".to_owned(), Value::String(package_type));
    entry.insert(
        "display_name".to_owned(),
        Value::String(localized(package.get("display_name"), &id, language)),
    );
    entry.insert(
        "description".to_owned(),
        Value::String(localized(package.get("short_description"), "", language)),
    );
    for key in [
        "author",
        "version",
        "categories",
        "min_app_version",
        "metadata_url",
        "metadata_sha256",
    ] {
        entry.insert(key.to_owned(), copy_value(package, key));
    }
    entry.insert("_sources".to_owned(), Value::Object(sources.clone()));
    entry.insert(
        "_primary_repo".to_owned(),
        Value::String(repository_url.clone()),
    );
    let installed_version = installed_version(installed, &id, app_version);
    if !installed_version.is_empty() {
        entry.insert(
            "installed_version".to_owned(),
            Value::String(installed_version),
        );
    }
    entry.insert("latest_version".to_owned(), Value::String(version.clone()));

    let Some(existing) = catalog
        .iter_mut()
        .find(|existing| text(existing.get("id")) == id)
    else {
        catalog.push(entry);
        return catalog;
    };

    let mut existing_sources = existing
        .get("_sources")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    existing_sources.extend(sources);
    existing.insert("_sources".to_owned(), Value::Object(existing_sources));

    let existing_primary = text(existing.get("_primary_repo"));
    let existing_priority = resolved_repository_priority(repositories, &existing_primary);
    let existing_latest = text(existing.get("latest_version"));
    match compare_versions(&version, &existing_latest) {
        value if value > 0 => {
            existing.insert("latest_version".to_owned(), Value::String(version.clone()));
            existing.insert("version".to_owned(), Value::String(version));
            existing.insert("_primary_repo".to_owned(), Value::String(repository_url));
            existing.insert(
                "display_name".to_owned(),
                Value::String(localized(package.get("display_name"), &id, language)),
            );
            existing.insert(
                "description".to_owned(),
                Value::String(localized(package.get("short_description"), "", language)),
            );
            for key in [
                "author",
                "categories",
                "min_app_version",
                "metadata_url",
                "metadata_sha256",
            ] {
                existing.insert(key.to_owned(), copy_value(package, key));
            }
        }
        0 if new_repository_priority < existing_priority => {
            existing.insert("_primary_repo".to_owned(), Value::String(repository_url));
            existing.insert(
                "metadata_url".to_owned(),
                copy_value(package, "metadata_url"),
            );
            existing.insert(
                "metadata_sha256".to_owned(),
                copy_value(package, "metadata_sha256"),
            );
        }
        0 if existing_primary.is_empty() => {
            existing.insert("_primary_repo".to_owned(), Value::String(repository_url));
        }
        _ => {}
    }
    catalog
}

fn merge_catalog_packages(
    mut catalog: Vec<Map<String, Value>>,
    packages: &[Map<String, Value>],
    repository: &Map<String, Value>,
    repositories: &[Map<String, Value>],
    installed: &Map<String, Value>,
    language: &str,
    app_version: &str,
) -> Vec<Map<String, Value>> {
    for package in packages {
        catalog = merge_catalog_package(
            catalog,
            package,
            repository,
            repositories,
            installed,
            language,
            app_version,
        );
    }
    catalog
}

fn has_updates(catalog: &[Map<String, Value>]) -> bool {
    catalog.iter().any(|package| {
        let installed = text(package.get("installed_version"));
        let latest = text(package.get("latest_version"));
        !installed.is_empty() && !latest.is_empty() && compare_versions(&latest, &installed) > 0
    })
}

fn upgrade_ids(catalog: &[Map<String, Value>]) -> Vec<Value> {
    catalog
        .iter()
        .filter(|package| {
            let installed = text(package.get("installed_version"));
            let latest = text(package.get("latest_version"));
            !installed.is_empty() && !latest.is_empty() && compare_versions(&latest, &installed) > 0
        })
        .map(|package| Value::String(text(package.get("id"))))
        .collect()
}

fn filter_catalog(catalog: &[Map<String, Value>], filter: &str) -> Vec<Value> {
    catalog
        .iter()
        .filter(|package| {
            if filter == "installed" {
                !text(package.get("installed_version")).is_empty()
            } else {
                text(package.get("type")) == filter
            }
        })
        .cloned()
        .map(Value::Object)
        .collect()
}

fn find_package(
    catalog: &[Map<String, Value>],
    package_id: &str,
    source_repository: &str,
) -> Value {
    let mut fallback = None;
    for package in catalog {
        if text(package.get("id")) != package_id {
            continue;
        }
        if source_repository.is_empty() {
            return Value::Object(package.clone());
        }
        if text(package.get("_primary_repo")) == source_repository {
            return Value::Object(package.clone());
        }
        fallback.get_or_insert_with(|| package.clone());
    }
    fallback.map(Value::Object).unwrap_or(Value::Null)
}

fn set_installed(
    catalog: &mut [Map<String, Value>],
    package_id: &str,
    version: Option<&str>,
) -> bool {
    if let Some(package) = catalog
        .iter_mut()
        .find(|package| text(package.get("id")) == package_id)
    {
        return match version {
            Some(version)
                if package.get("installed_version").and_then(Value::as_str) != Some(version) =>
            {
                package.insert(
                    "installed_version".to_owned(),
                    Value::String(version.to_owned()),
                );
                true
            }
            Some(_) => false,
            None => package.remove("installed_version").is_some(),
        };
    }
    false
}

fn mutate_repositories(
    mut repositories: Vec<Map<String, Value>>,
    operation: &str,
    url: &str,
    enabled: bool,
    priority: i32,
) -> Option<(Vec<Value>, bool)> {
    let index = repositories
        .iter()
        .position(|repository| text(repository.get("url")) == url);
    let changed = match operation {
        "add" if index.is_none() => {
            repositories.push(
                json!({"url": url, "name": url, "enabled": enabled, "priority": priority})
                    .as_object()
                    .cloned()
                    .expect("repository fixture is an object"),
            );
            true
        }
        "remove" => {
            if let Some(index) = index {
                repositories.remove(index);
                true
            } else {
                false
            }
        }
        "enabled" => index.is_some_and(|index| {
            if boolean(repositories[index].get("enabled"), true) == enabled {
                false
            } else {
                repositories[index].insert("enabled".to_owned(), Value::Bool(enabled));
                true
            }
        }),
        "priority" => index.is_some_and(|index| {
            if integer(repositories[index].get("priority"), 10) == priority {
                false
            } else {
                repositories[index].insert("priority".to_owned(), json!(priority));
                true
            }
        }),
        "add" => false,
        _ => return None,
    };
    Some((
        repositories.into_iter().map(Value::Object).collect(),
        changed,
    ))
}

fn object(value: Value) -> Option<Map<String, Value>> {
    match value {
        Value::Object(value) => Some(value),
        _ => None,
    }
}

fn enabled_repositories(mut repositories: Vec<Map<String, Value>>) -> Vec<Value> {
    repositories.retain(|repository| boolean(repository.get("enabled"), true));
    repositories.sort_by_key(|repository| integer(repository.get("priority"), 10));
    repositories.into_iter().map(Value::Object).collect()
}

fn normalize_metadata(detail: &Map<String, Value>) -> Option<Map<String, Value>> {
    let package_type = text(detail.get("type"));
    if !supported_package_type(&package_type) {
        return None;
    }
    if let Some(versions) = detail.get("versions") {
        let versions = versions.as_array()?;
        if versions.iter().any(|version| !version.is_object()) {
            return None;
        }
    }
    Some(detail.clone())
}

fn select_install(
    detail: &Map<String, Value>,
    requested_version: &str,
    app_version: &str,
) -> Value {
    let mut target_version = requested_version.to_owned();
    let (download_url, sha256, minimum_app_version) = match objects(detail.get("versions")) {
        versions if versions.is_empty() => {
            if target_version.is_empty() {
                target_version = text(detail.get("version"));
            }
            (
                text(detail.get("download_url")),
                text(detail.get("download_sha256")),
                text(detail.get("min_app_version")),
            )
        }
        versions if target_version.is_empty() => {
            let best = versions
                .into_iter()
                .reduce(|best, candidate| {
                    if compare_versions(&text(candidate.get("version")), &text(best.get("version")))
                        > 0
                    {
                        candidate
                    } else {
                        best
                    }
                })
                .unwrap_or_default();
            target_version = text(best.get("version"));
            (
                text(best.get("download_url")),
                text(best.get("download_sha256")),
                text(best.get("min_app_version")),
            )
        }
        versions => {
            let selected = versions
                .into_iter()
                .find(|version| text(version.get("version")) == target_version)
                .unwrap_or_default();
            (
                text(selected.get("download_url")),
                text(selected.get("download_sha256")),
                text(selected.get("min_app_version")),
            )
        }
    };
    let package_type = text(detail.get("type"));
    let status = if !supported_package_type(&package_type) {
        "invalid_type"
    } else if download_url.is_empty() {
        "no_download"
    } else if !minimum_app_version.is_empty()
        && compare_versions(app_version, &minimum_app_version) < 0
    {
        "requires_newer_app"
    } else {
        "ok"
    };
    json!({
        "status": status,
        "version": target_version,
        "downloadUrl": download_url,
        "sha256": sha256,
        "minAppVersion": minimum_app_version,
        "type": package_type
    })
}

fn apply(input: &Map<String, Value>) -> Option<Map<String, Value>> {
    let operation = text(input.get("operation"));
    match operation.as_str() {
        "repositories" => {
            let (repositories, changed) = mutate_repositories(
                objects(input.get("repositories")),
                &text(input.get("repositoryOperation")),
                &text(input.get("url")),
                boolean(input.get("enabled"), true),
                integer(input.get("priority"), 10),
            )?;
            object(json!({"repositories": repositories, "changed": changed}))
        }
        "enabledRepositories" => object(
            json!({"repositories": enabled_repositories(objects(input.get("repositories")))}),
        ),
        "normalizeMetadata" => {
            let detail = normalize_metadata(input.get("detail")?.as_object()?)?;
            object(json!({"detail": detail}))
        }
        "selectInstall" => object(json!({
            "selection": select_install(
                input.get("detail")?.as_object()?,
                &text(input.get("requestedVersion")),
                &text(input.get("appVersion"))
            )
        })),
        _ => None,
    }
}

pub struct AviQtlPackageCatalogState {
    catalog: Mutex<Vec<Map<String, Value>>>,
}

fn with_catalog<T>(
    handle: *const AviQtlPackageCatalogState,
    operation: impl FnOnce(&[Map<String, Value>]) -> T,
) -> Option<T> {
    // SAFETY: Non-null handles are required to originate from the catalog-state create function.
    let state = unsafe { handle.as_ref() }?;
    let catalog = state.catalog.lock().ok()?;
    Some(operation(&catalog))
}

fn with_catalog_mut<T>(
    handle: *mut AviQtlPackageCatalogState,
    operation: impl FnOnce(&mut Vec<Map<String, Value>>) -> T,
) -> Option<T> {
    // SAFETY: Non-null handles are required to originate from the catalog-state create function.
    let state = unsafe { handle.as_ref() }?;
    let mut catalog = state.catalog.lock().ok()?;
    Some(operation(&mut catalog))
}

unsafe fn input_bytes<'a>(input: *const u8, input_length: usize) -> &'a [u8] {
    if input_length == 0 {
        &[]
    } else {
        // SAFETY: Callers validate the full input range before invoking this helper.
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
    value: &impl serde::Serialize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let Ok(json) = serde_json::to_vec(value) else {
        return STATUS_INVALID_JSON;
    };
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

/// Creates an empty Rust-owned package catalog.
///
/// # Safety
///
/// `output_handle` must point to writable storage for one handle. A returned handle must be
/// destroyed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_create(
    output_handle: *mut *mut AviQtlPackageCatalogState,
) -> u32 {
    if !slice_is_valid(output_handle, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let handle = Box::into_raw(Box::new(AviQtlPackageCatalogState {
        catalog: Mutex::new(Vec::new()),
    }));
    // SAFETY: The output handle was validated above.
    unsafe { output_handle.write(handle) };
    STATUS_OK
}

/// Destroys Rust-owned package catalog state. A null handle is accepted.
///
/// # Safety
///
/// A non-null handle must have been returned by the catalog-state create function exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_destroy(
    handle: *mut AviQtlPackageCatalogState,
) {
    if !handle.is_null() {
        // SAFETY: The caller guarantees unique ownership of one live handle.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Clears all package catalog entries.
///
/// # Safety
///
/// The handle must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_clear(
    handle: *mut AviQtlPackageCatalogState,
) -> u32 {
    if handle.is_null() {
        return STATUS_INVALID_ARGUMENT;
    }
    with_catalog_mut(handle, Vec::clear)
        .map(|()| STATUS_OK)
        .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Merges one repository package batch into the package catalog.
///
/// # Safety
///
/// The handle must be live and the input range must contain a readable UTF-8 JSON object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_merge_json(
    handle: *mut AviQtlPackageCatalogState,
    input: *const u8,
    input_length: usize,
) -> u32 {
    if handle.is_null() || !slice_is_valid(input, input_length) {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: The input range was validated above.
    let Some(input) = serde_json::from_slice::<Value>(unsafe { input_bytes(input, input_length) })
        .ok()
        .and_then(|value| value.as_object().cloned())
    else {
        return STATUS_INVALID_JSON;
    };
    let packages = objects(input.get("packages"));
    let Some(repository) = input.get("repository").and_then(Value::as_object) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let repositories = objects(input.get("repositories"));
    let Some(installed) = input.get("installed").and_then(Value::as_object) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let language = text(input.get("language"));
    let app_version = text(input.get("appVersion"));
    with_catalog_mut(handle, |catalog| {
        *catalog = merge_catalog_packages(
            std::mem::take(catalog),
            &packages,
            repository,
            &repositories,
            installed,
            &language,
            &app_version,
        );
    })
    .map(|()| STATUS_OK)
    .unwrap_or(STATUS_INVALID_ARGUMENT)
}

unsafe fn catalog_json(
    handle: *const AviQtlPackageCatalogState,
    value: impl FnOnce(&[Map<String, Value>]) -> Value,
    inputs: &[(*const u8, usize)],
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
        let value = value(catalog);
        // SAFETY: Output ranges were validated and checked for overlap above.
        unsafe { write_json(&value, output, output_capacity, output_length) }
    })
    .unwrap_or(STATUS_INVALID_ARGUMENT)
}

/// Serializes the complete package catalog as a JSON array.
///
/// # Safety
///
/// The handle must be live. Output ranges must be writable, valid, and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_snapshot_json(
    handle: *const AviQtlPackageCatalogState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: This function forwards the caller's state/output contract unchanged.
    unsafe {
        catalog_json(
            handle,
            |catalog| Value::Array(catalog.iter().cloned().map(Value::Object).collect()),
            &[],
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Reports whether any installed package has a newer catalog version.
///
/// # Safety
///
/// The handle must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_has_updates(
    handle: *const AviQtlPackageCatalogState,
) -> u32 {
    with_catalog(handle, has_updates)
        .map(u32::from)
        .unwrap_or(0)
}

/// Serializes the matching package as a JSON object, or an empty object when absent.
///
/// # Safety
///
/// The handle and input ranges must be valid. Output ranges must be writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_find_json(
    handle: *const AviQtlPackageCatalogState,
    package_id: *const u8,
    package_id_length: usize,
    source_repository: *const u8,
    source_repository_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let package_id_input = package_id;
    let source_repository_input = source_repository;
    let Some(package_id) = (unsafe { crate::abi::utf8(package_id_input, package_id_length) })
    else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(source_repository) =
        (unsafe { crate::abi::utf8(source_repository_input, source_repository_length) })
    else {
        return STATUS_INVALID_ARGUMENT;
    };
    let package_id = package_id.to_owned();
    let source_repository = source_repository.to_owned();
    // SAFETY: The input strings are now owned and the original ranges are used only for overlap validation.
    unsafe {
        catalog_json(
            handle,
            |catalog| match find_package(catalog, &package_id, &source_repository) {
                Value::Object(package) => Value::Object(package),
                _ => Value::Object(Map::new()),
            },
            &[
                (package_id_input, package_id_length),
                (source_repository_input, source_repository_length),
            ],
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Serializes catalog entries matching a type/filter as a JSON array.
///
/// # Safety
///
/// The handle and filter range must be valid. Output ranges must be writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_filter_json(
    handle: *const AviQtlPackageCatalogState,
    filter: *const u8,
    filter_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    let filter_input = filter;
    let Some(filter) = (unsafe { crate::abi::utf8(filter_input, filter_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let filter = filter.to_owned();
    // SAFETY: The filter is now owned and the original range is used only for overlap validation.
    unsafe {
        catalog_json(
            handle,
            |catalog| Value::Array(filter_catalog(catalog, &filter)),
            &[(filter_input, filter_length)],
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Serializes package IDs eligible for upgrade as a JSON array.
///
/// # Safety
///
/// The handle must be live. Output ranges must be writable, valid, and non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_upgrade_ids_json(
    handle: *const AviQtlPackageCatalogState,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    // SAFETY: This function forwards the caller's state/output contract unchanged.
    unsafe {
        catalog_json(
            handle,
            |catalog| Value::Array(upgrade_ids(catalog)),
            &[],
            output,
            output_capacity,
            output_length,
        )
    }
}

/// Updates the installed version projection for one package.
///
/// # Safety
///
/// The handle and string ranges must be valid. `changed` must be writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_catalog_state_set_installed(
    handle: *mut AviQtlPackageCatalogState,
    package_id: *const u8,
    package_id_length: usize,
    version: *const u8,
    version_length: usize,
    installed: u32,
    changed: *mut u32,
) -> u32 {
    if handle.is_null() || !slice_is_valid(changed, 1) {
        return STATUS_INVALID_ARGUMENT;
    }
    let Some(package_id) = (unsafe { crate::abi::utf8(package_id, package_id_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Some(version) = (unsafe { crate::abi::utf8(version, version_length) }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let overlaps = [
        ranges_overlap(package_id.as_ptr(), package_id_length, changed, 1),
        ranges_overlap(version.as_ptr(), version_length, changed, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let package_id = package_id.to_owned();
    let version = (installed != 0).then(|| version.to_owned());
    let Some(was_changed) = with_catalog_mut(handle, |catalog| {
        set_installed(catalog, &package_id, version.as_deref())
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    // SAFETY: The output flag was validated and checked against the string ranges.
    unsafe { changed.write(u32::from(was_changed)) };
    STATUS_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_package_document_apply_json(
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
        // SAFETY: The input range was validated and checked for output overlap.
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
    // SAFETY: The output-length range was validated and de-overlapped above.
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

    fn collect_json(mut function: impl FnMut(*mut u8, usize, *mut usize) -> u32) -> Value {
        let mut required = 0;
        assert_eq!(
            function(std::ptr::null_mut(), 0, &mut required),
            STATUS_BUFFER_TOO_SMALL
        );
        let mut output = vec![0_u8; required];
        let mut written = 0;
        assert_eq!(
            function(output.as_mut_ptr(), output.len(), &mut written),
            STATUS_OK
        );
        assert_eq!(written, output.len());
        serde_json::from_slice(&output).expect("state output")
    }

    #[test]
    fn version_comparison_preserves_existing_component_rules() {
        assert_eq!(compare_versions("v1.2.0", "1.1.9"), 1);
        assert_eq!(compare_versions("2.0.0-rc.1", "2.0.0"), -1);
        assert_eq!(compare_versions("2.0.0+build.1", "2.0.0+build.2"), 0);
        assert_eq!(compare_versions("1.0", "1"), 1);
        assert_eq!(compare_versions("1.beta", "1.alpha"), 1);
        assert_eq!(compare_versions("1.0", "1.0"), 0);
    }

    #[test]
    fn catalog_merge_uses_version_then_repository_priority() {
        let package = json!({
            "id": "org.aviqtl.effect",
            "type": "effect",
            "version": "1.0.0",
            "display_name": {"en": "Effect"},
            "metadata_url": "https://low/metadata.json"
        });
        let repositories = objects(Some(&json!([
            {"url": "https://high", "priority": 1},
            {"url": "https://low", "priority": 10}
        ])));
        assert_eq!(
            resolved_repository_priority(&repositories, "https://high"),
            1
        );
        assert_eq!(
            resolved_repository_priority(&repositories, "https://unknown"),
            i32::MAX
        );
        let catalog = merge_catalog_package(
            Vec::new(),
            package.as_object().expect("fixture"),
            json!({"url": "https://low", "priority": 10})
                .as_object()
                .expect("fixture"),
            &repositories,
            &Map::new(),
            "en",
            "0.5.9",
        );
        let newer = json!({
            "id": "org.aviqtl.effect",
            "type": "effect",
            "version": "2.0.0",
            "display_name": {"en": "New Effect"},
            "short_description": {"en": "New description"},
            "author": "New Author",
            "categories": ["New"],
            "min_app_version": "0.6.0",
            "metadata_url": "https://high/metadata.json",
            "metadata_sha256": "abcd"
        });
        let catalog = merge_catalog_package(
            catalog,
            newer.as_object().expect("fixture"),
            json!({"url": "https://high", "priority": 1})
                .as_object()
                .expect("fixture"),
            &repositories,
            &Map::new(),
            "en",
            "0.5.9",
        );
        assert_eq!(text(catalog[0].get("latest_version")), "2.0.0");
        assert_eq!(text(catalog[0].get("_primary_repo")), "https://high");
        assert_eq!(text(catalog[0].get("display_name")), "New Effect");
        assert_eq!(text(catalog[0].get("description")), "New description");
        assert_eq!(text(catalog[0].get("author")), "New Author");
        assert_eq!(text(catalog[0].get("min_app_version")), "0.6.0");
        assert_eq!(
            catalog[0]
                .get("_sources")
                .and_then(Value::as_object)
                .map(Map::len),
            Some(2)
        );

        let priority_catalog = merge_catalog_package(
            Vec::new(),
            package.as_object().expect("fixture"),
            json!({"url": "https://low"}).as_object().expect("fixture"),
            &repositories,
            &Map::new(),
            "en",
            "0.5.9",
        );
        let same_version = json!({
            "id": "org.aviqtl.effect",
            "type": "effect",
            "version": "1.0.0",
            "metadata_url": "https://high/metadata.json"
        });
        let priority_catalog = merge_catalog_package(
            priority_catalog,
            same_version.as_object().expect("fixture"),
            json!({"url": "https://high"}).as_object().expect("fixture"),
            &repositories,
            &Map::new(),
            "en",
            "0.5.9",
        );
        assert_eq!(
            text(priority_catalog[0].get("_primary_repo")),
            "https://high"
        );
    }

    #[test]
    fn catalog_batch_merges_every_package_in_one_operation() {
        let packages = objects(Some(&json!([
            {"id": "org.aviqtl.one", "type": "effect", "version": "1.0.0"},
            {"id": "org.aviqtl.two", "type": "object", "version": "2.0.0"}
        ])));
        let repository = json!({"url": "https://repo", "priority": 1});
        let catalog = merge_catalog_packages(
            Vec::new(),
            &packages,
            repository.as_object().expect("fixture"),
            &objects(Some(&json!([{"url": "https://repo", "priority": 1}]))),
            &Map::new(),
            "en",
            "0.5.9",
        );
        assert_eq!(catalog.len(), 2);
        assert_eq!(text(catalog[0].get("id")), "org.aviqtl.one");
        assert_eq!(text(catalog[1].get("id")), "org.aviqtl.two");
    }

    #[test]
    fn install_selection_chooses_latest_and_enforces_minimum_app_version() {
        let detail = json!({
            "type": "effect",
            "versions": [
                {"version": "1.0.0", "download_url": "https://one"},
                {"version": "2.0.0", "download_url": "https://two", "min_app_version": "0.6.0"}
            ]
        });
        let selection = select_install(detail.as_object().expect("fixture"), "", "0.5.9");
        assert_eq!(
            selection.get("status").and_then(Value::as_str),
            Some("requires_newer_app")
        );
        assert_eq!(
            selection.get("version").and_then(Value::as_str),
            Some("2.0.0")
        );
    }

    #[test]
    fn repository_mutations_are_idempotent() {
        let (repositories, changed) =
            mutate_repositories(Vec::new(), "add", "https://example/repo.json", true, 5).unwrap();
        assert!(changed);
        let repositories = objects(Some(&Value::Array(repositories)));
        let (_, changed) = mutate_repositories(
            repositories.clone(),
            "add",
            "https://example/repo.json",
            true,
            5,
        )
        .unwrap();
        assert!(!changed);
        assert!(mutate_repositories(repositories, "typo", "", true, 10).is_none());

        let enabled = enabled_repositories(objects(Some(&json!([
            {"url": "disabled", "enabled": false, "priority": 0},
            {"url": "later", "enabled": true, "priority": 20},
            {"url": "first", "enabled": true, "priority": 1}
        ]))));
        assert_eq!(
            enabled
                .iter()
                .filter_map(Value::as_object)
                .map(|repository| text(repository.get("url")))
                .collect::<Vec<_>>(),
            ["first", "later"]
        );
    }

    #[test]
    fn catalog_query_operations_cover_user_visible_results() {
        let catalog = objects(Some(&json!([
            {
                "id": "effect.one",
                "type": "effect",
                "installed_version": "1.0.0",
                "latest_version": "2.0.0",
                "_primary_repo": "https://one"
            },
            {"id": "object.two", "type": "object", "latest_version": "1.0.0"}
        ])));

        assert_eq!(filter_catalog(&catalog, "effect").len(), 1);
        assert!(filter_catalog(&catalog, "").is_empty());
        assert_eq!(filter_catalog(&catalog, "installed").len(), 1);
        assert_eq!(
            find_package(&catalog, "effect.one", "https://missing")
                .get("id")
                .and_then(Value::as_str),
            Some("effect.one")
        );

        let mut removed = catalog.clone();
        assert!(set_installed(&mut removed, "effect.one", None));
        assert!(removed[0].get("installed_version").is_none());
        let mut installed = catalog.clone();
        assert!(set_installed(&mut installed, "object.two", Some("1.0.0")));
        assert_eq!(
            installed[1]
                .get("installed_version")
                .and_then(Value::as_str),
            Some("1.0.0")
        );

        let normalized = normalize_metadata(
            json!({"type": "effect", "versions": [{"version": "1.0.0"}]})
                .as_object()
                .expect("fixture"),
        );
        assert!(normalized.is_some());
        assert!(
            normalize_metadata(json!({"type": "unknown"}).as_object().expect("fixture")).is_none()
        );
        assert!(has_updates(&catalog));
        assert_eq!(upgrade_ids(&catalog), [json!("effect.one")]);
    }

    #[test]
    fn catalog_state_owns_merge_queries_and_installed_projection() {
        let mut handle = std::ptr::null_mut();
        // SAFETY: The handle-output range is writable and receives one owned state handle.
        assert_eq!(
            unsafe { aviqtl_package_catalog_state_create(&mut handle) },
            STATUS_OK
        );
        assert!(!handle.is_null());

        let merge = br#"{
            "packages":[{
                "id":"effect.one",
                "type":"effect",
                "version":"2.0.0",
                "display_name":{"en":"Effect One"},
                "metadata_url":"https://repo/effect.json"
            }],
            "repository":{"url":"https://repo"},
            "repositories":[{"url":"https://repo","priority":1}],
            "installed":{"effect.one":{"version":"1.0.0"}},
            "language":"en",
            "appVersion":"0.5.9"
        }"#;
        // SAFETY: The state handle and immutable JSON byte range are valid.
        assert_eq!(
            unsafe { aviqtl_package_catalog_state_merge_json(handle, merge.as_ptr(), merge.len()) },
            STATUS_OK
        );
        // SAFETY: The state handle remains live.
        assert_eq!(
            unsafe { aviqtl_package_catalog_state_has_updates(handle) },
            1
        );

        let snapshot = collect_json(|output, capacity, length| {
            // SAFETY: The live handle and callback-provided output ranges satisfy the contract.
            unsafe { aviqtl_package_catalog_state_snapshot_json(handle, output, capacity, length) }
        });
        assert_eq!(snapshot[0]["id"], "effect.one");
        assert_eq!(snapshot[0]["installed_version"], "1.0.0");

        let package_id = b"effect.one";
        let repository = b"https://repo";
        let found = collect_json(|output, capacity, length| {
            // SAFETY: String and output ranges are valid and disjoint.
            unsafe {
                aviqtl_package_catalog_state_find_json(
                    handle,
                    package_id.as_ptr(),
                    package_id.len(),
                    repository.as_ptr(),
                    repository.len(),
                    output,
                    capacity,
                    length,
                )
            }
        });
        assert_eq!(found["display_name"], "Effect One");

        let filter = b"installed";
        let installed = collect_json(|output, capacity, length| {
            // SAFETY: String and output ranges are valid and disjoint.
            unsafe {
                aviqtl_package_catalog_state_filter_json(
                    handle,
                    filter.as_ptr(),
                    filter.len(),
                    output,
                    capacity,
                    length,
                )
            }
        });
        assert_eq!(installed.as_array().map(Vec::len), Some(1));

        let version = b"2.0.0";
        let mut changed = 0;
        // SAFETY: The live handle, strings, and changed flag are valid and disjoint.
        assert_eq!(
            unsafe {
                aviqtl_package_catalog_state_set_installed(
                    handle,
                    package_id.as_ptr(),
                    package_id.len(),
                    version.as_ptr(),
                    version.len(),
                    1,
                    &mut changed,
                )
            },
            STATUS_OK
        );
        assert_eq!(changed, 1);
        // SAFETY: The state handle remains live.
        assert_eq!(
            unsafe { aviqtl_package_catalog_state_has_updates(handle) },
            0
        );

        let before_invalid = collect_json(|output, capacity, length| {
            // SAFETY: The live handle and callback-provided output ranges satisfy the contract.
            unsafe { aviqtl_package_catalog_state_snapshot_json(handle, output, capacity, length) }
        });
        let invalid = b"not-json";
        // SAFETY: The state handle and immutable byte range are valid.
        assert_eq!(
            unsafe {
                aviqtl_package_catalog_state_merge_json(handle, invalid.as_ptr(), invalid.len())
            },
            STATUS_INVALID_JSON
        );
        let after_invalid = collect_json(|output, capacity, length| {
            // SAFETY: The live handle and callback-provided output ranges satisfy the contract.
            unsafe { aviqtl_package_catalog_state_snapshot_json(handle, output, capacity, length) }
        });
        assert_eq!(after_invalid, before_invalid);

        // SAFETY: The handle is destroyed exactly once at the end of the test.
        unsafe { aviqtl_package_catalog_state_destroy(handle) };
    }
}
