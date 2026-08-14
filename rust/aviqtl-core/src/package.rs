use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use crate::policy::valid_package_id;
use semver::Version;
use serde_json::{Map, Value, json};

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

fn compare_versions(left: &str, right: &str) -> i32 {
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
        }
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

fn repository_priority(repositories: &[Map<String, Value>], url: &str) -> Option<i32> {
    repositories
        .iter()
        .find(|repository| text(repository.get("url")) == url)
        .map(|repository| integer(repository.get("priority"), 10))
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
    let new_repository_priority = repository_priority(repositories, &repository_url)
        .unwrap_or_else(|| integer(repository.get("priority"), 10));
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
    let existing_priority =
        repository_priority(repositories, &existing_primary).unwrap_or(i32::MAX);
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
    mut catalog: Vec<Map<String, Value>>,
    package_id: &str,
    version: Option<&str>,
) -> Vec<Value> {
    if let Some(package) = catalog
        .iter_mut()
        .find(|package| text(package.get("id")) == package_id)
    {
        if let Some(version) = version {
            package.insert(
                "installed_version".to_owned(),
                Value::String(version.to_owned()),
            );
        } else {
            package.remove("installed_version");
        }
    }
    catalog.into_iter().map(Value::Object).collect()
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
    let catalog = objects(input.get("catalog"));
    match operation.as_str() {
        "mergeCatalog" => {
            let package = input.get("package")?.as_object()?;
            let repository = input.get("repository")?.as_object()?;
            let repositories = objects(input.get("repositories"));
            let installed = input.get("installed")?.as_object()?;
            let language = text(input.get("language"));
            let app_version = text(input.get("appVersion"));
            object(json!({
                "catalog": merge_catalog_package(catalog, package, repository, &repositories, installed, &language, &app_version)
            }))
        }
        "mergeCatalogBatch" => {
            let packages = objects(input.get("packages"));
            let repository = input.get("repository")?.as_object()?;
            let repositories = objects(input.get("repositories"));
            let installed = input.get("installed")?.as_object()?;
            let language = text(input.get("language"));
            let app_version = text(input.get("appVersion"));
            object(json!({
                "catalog": merge_catalog_packages(
                    catalog,
                    &packages,
                    repository,
                    &repositories,
                    installed,
                    &language,
                    &app_version,
                )
            }))
        }
        "compareVersions" => object(
            json!({"value": compare_versions(&text(input.get("left")), &text(input.get("right")))}),
        ),
        "hasUpdates" => object(json!({"value": has_updates(&catalog)})),
        "upgradeIds" => object(json!({"ids": upgrade_ids(&catalog)})),
        "filter" => object(json!({
            "catalog": filter_catalog(&catalog, &text(input.get("filter")))
        })),
        "find" => object(json!({
            "package": find_package(&catalog, &text(input.get("packageId")), &text(input.get("sourceRepository")))
        })),
        "setInstalled" => object(json!({
            "catalog": set_installed(
                catalog,
                &text(input.get("packageId")),
                input.get("version").and_then(Value::as_str)
            )
        })),
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

        let removed = set_installed(catalog.clone(), "effect.one", None);
        assert!(removed[0].get("installed_version").is_none());
        let installed = set_installed(catalog.clone(), "object.two", Some("1.0.0"));
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
}
