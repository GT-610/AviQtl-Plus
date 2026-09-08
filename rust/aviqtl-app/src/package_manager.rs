use crate::project_io::write_atomic;
use crate::settings::{SettingsStore, package_paths};
use aviqtl_rust_core::api::{
    PackageCatalog, PackageRepositoryOperation, PluginPermission, PluginPermissionState,
    enabled_package_repositories, mutate_package_repositories, normalize_package_metadata,
    package_archive_path_is_safe, package_id_is_valid, package_type_is_installable,
    select_package_install,
};
use crc32fast::Hasher as Crc32;
use flate2::read::DeflateDecoder;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

const MAX_REPOSITORY_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PACKAGE_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PACKAGE_EXTRACTED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_PACKAGE_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_INSTALLED_STATE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PLUGIN_SCRIPT_BYTES: u64 = 8 * 1024 * 1024;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageOperation {
    Sync,
    Install {
        package_id: String,
        source_repository: String,
        version: String,
    },
    Remove {
        package_id: String,
    },
    UpgradeAll,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageOperationOutcome {
    pub installed: Vec<String>,
    pub removed: Vec<String>,
    pub errors: Vec<String>,
    pub reload_effect_catalog: bool,
    pub self_update_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPermissionGrant {
    pub name: String,
    pub granted: bool,
}

pub fn plugin_permission_grants(
    settings: &SettingsStore,
    plugin_id: &str,
) -> Vec<PluginPermissionGrant> {
    let state = settings
        .value("pluginPermissions")
        .and_then(PluginPermissionState::from_value)
        .unwrap_or_default();
    PluginPermission::ALL
        .into_iter()
        .map(|permission| PluginPermissionGrant {
            name: permission.name().to_owned(),
            granted: state.has(plugin_id, permission),
        })
        .collect()
}

pub fn save_plugin_permission_grants(
    settings: &mut SettingsStore,
    plugin_id: &str,
    granted_names: &[String],
) -> Result<(), String> {
    let mut state = settings
        .value("pluginPermissions")
        .and_then(PluginPermissionState::from_value)
        .unwrap_or_default();
    state.revoke_all(plugin_id);
    for name in granted_names {
        if let Some(permission) = PluginPermission::from_name(name) {
            state.set(plugin_id, permission, true);
        }
    }
    let mut replacement = settings.snapshot();
    replacement.insert(
        "pluginPermissions".to_owned(),
        Value::Object(state.snapshot()),
    );
    settings.apply(replacement)
}

pub trait PackageHttpClient: Send + Sync {
    fn get(&self, url: &str, maximum_bytes: u64) -> Result<Vec<u8>, String>;
}

#[derive(Clone)]
pub struct UreqPackageHttpClient {
    agent: ureq::Agent,
}

impl Default for UreqPackageHttpClient {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(NETWORK_TIMEOUT)
                .timeout_read(NETWORK_TIMEOUT)
                .timeout_write(NETWORK_TIMEOUT)
                .redirects(0)
                .build(),
        }
    }
}

impl PackageHttpClient for UreqPackageHttpClient {
    fn get(&self, url: &str, maximum_bytes: u64) -> Result<Vec<u8>, String> {
        let mut current = Url::parse(url).map_err(|error| error.to_string())?;
        if !secure_network_url(current.as_str()) {
            return Err(format!("URL must use HTTPS: {url}"));
        }
        let mut redirect_count = 0_u8;
        let response = loop {
            let response = self
                .agent
                .get(current.as_str())
                .call()
                .map_err(|error| error.to_string())?;
            if !(300..400).contains(&response.status()) {
                break response;
            }
            let location = response
                .header("Location")
                .ok_or_else(|| format!("Redirect from {current} did not include a location"))?;
            let next = current
                .join(location)
                .map_err(|error| format!("Invalid redirect from {current}: {error}"))?;
            if !secure_network_url(next.as_str()) {
                return Err(format!("Redirect must remain on HTTPS: {next}"));
            }
            if redirect_count >= 5 {
                return Err(format!("Too many redirects while fetching {url}"));
            }
            redirect_count += 1;
            current = next;
        };
        if response
            .header("Content-Length")
            .and_then(|length| length.parse::<u64>().ok())
            .is_some_and(|length| length > maximum_bytes)
        {
            return Err(format!(
                "Response exceeds the maximum allowed size of {maximum_bytes} bytes"
            ));
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(maximum_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > maximum_bytes {
            return Err(format!(
                "Response exceeds the maximum allowed size of {maximum_bytes} bytes"
            ));
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageSyncOutcome {
    pub repositories_synced: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSection {
    Effect,
    Object,
    Mod,
    Installed,
    Application,
}

impl PackageSection {
    fn package_type(self) -> &'static str {
        match self {
            Self::Effect => "effect",
            Self::Object => "object",
            Self::Mod => "mod",
            Self::Installed => "installed",
            Self::Application => "application",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageListItem {
    pub id: String,
    pub package_type: String,
    pub display_name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub installed_version: String,
    pub latest_version: String,
    pub source_repository: String,
    pub local_file_plugin: bool,
    pub has_update: bool,
}

impl PackageListItem {
    pub fn is_installed(&self) -> bool {
        !self.installed_version.is_empty()
    }

    pub fn can_manage_permissions(&self) -> bool {
        self.is_installed() && self.package_type == "mod"
    }

    pub fn can_remove(&self) -> bool {
        self.is_installed() && self.id != "org.aviqtl.app" && !self.local_file_plugin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRepositoryItem {
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub priority: i32,
}

#[derive(Debug, Clone)]
pub struct PackageManagerModel {
    root: PathBuf,
    app_version: String,
    language: String,
    repositories: Vec<Value>,
    installed: Map<String, Value>,
    catalog: PackageCatalog,
    package_details: BTreeMap<String, Map<String, Value>>,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageInstallOutcome {
    Installed {
        package_id: String,
        package_type: String,
    },
    SelfUpdate {
        version: String,
    },
}

impl PackageManagerModel {
    pub fn load(settings: &SettingsStore) -> Self {
        Self::load_from(
            default_package_root(),
            configured_repositories(settings),
            system_language(),
            env!("CARGO_PKG_VERSION").to_owned(),
        )
    }

    fn load_from(
        root: PathBuf,
        repositories: Vec<Value>,
        language: String,
        app_version: String,
    ) -> Self {
        let mut model = Self {
            root,
            app_version,
            language,
            repositories,
            installed: Map::new(),
            catalog: PackageCatalog::default(),
            package_details: BTreeMap::new(),
            status: String::new(),
        };
        model.reload_cached_packages();
        model
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn has_updates(&self) -> bool {
        self.catalog.has_updates()
    }

    pub fn repositories(&self) -> Vec<PackageRepositoryItem> {
        self.repositories
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|repository| {
                let url = text(repository.get("url"));
                (!url.is_empty()).then(|| PackageRepositoryItem {
                    name: text(repository.get("name")),
                    url,
                    enabled: repository
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    priority: repository
                        .get("priority")
                        .and_then(Value::as_i64)
                        .and_then(|priority| i32::try_from(priority).ok())
                        .unwrap_or(10),
                })
            })
            .collect()
    }

    pub fn enabled_repositories(&self) -> Vec<Value> {
        enabled_package_repositories(&self.repositories)
    }

    pub fn refresh_repositories<C: PackageHttpClient>(&mut self, client: &C) -> PackageSyncOutcome {
        self.catalog.clear();
        self.installed =
            read_json_object(&self.root.join("installed.json"), MAX_INSTALLED_STATE_BYTES)
                .unwrap_or_default();
        self.installed.insert(
            "org.aviqtl.app".to_owned(),
            serde_json::json!({"version": self.app_version}),
        );
        let repositories = self.enabled_repositories();
        let mut outcome = PackageSyncOutcome::default();
        for repository in repositories {
            let Some(mut repository_info) = repository.as_object().cloned() else {
                continue;
            };
            let repository_url = text(repository_info.get("url"));
            match self.fetch_repository(client, &repository_url, &mut repository_info) {
                Ok(()) => outcome.repositories_synced += 1,
                Err(error) => outcome.errors.push(error),
            }
        }
        self.status = "Sync complete".to_owned();
        outcome
    }

    pub fn execute_operation<C, F>(
        &mut self,
        operation: PackageOperation,
        client: &C,
        mut progress: F,
    ) -> PackageOperationOutcome
    where
        C: PackageHttpClient,
        F: FnMut(&str, f32),
    {
        let mut outcome = PackageOperationOutcome::default();
        match operation {
            PackageOperation::Sync => {
                progress("Syncing repository...", 0.0);
                let sync = self.refresh_repositories(client);
                outcome.errors = sync.errors;
                progress(self.status(), 1.0);
            }
            PackageOperation::Install {
                package_id,
                source_repository,
                version,
            } => {
                match self.install_package(
                    client,
                    &package_id,
                    &source_repository,
                    &version,
                    &mut progress,
                ) {
                    Ok(PackageInstallOutcome::Installed {
                        package_id,
                        package_type,
                    }) => {
                        outcome.reload_effect_catalog =
                            matches!(package_type.as_str(), "effect" | "object");
                        outcome.installed.push(package_id);
                    }
                    Ok(PackageInstallOutcome::SelfUpdate { version }) => {
                        outcome.self_update_version = Some(version);
                    }
                    Err(error) => {
                        self.status = "Installation failed".to_owned();
                        outcome.errors.push(error);
                    }
                }
                progress(self.status(), 1.0);
            }
            PackageOperation::Remove { package_id } => {
                progress(&format!("Removing package: {package_id}"), 0.0);
                match self.remove_installed_package(&package_id) {
                    Ok(package_type) => {
                        outcome.reload_effect_catalog =
                            matches!(package_type.as_str(), "effect" | "object");
                        outcome.removed.push(package_id);
                    }
                    Err(error) => {
                        self.status = "Removal failed".to_owned();
                        outcome.errors.push(error);
                    }
                }
                progress(self.status(), 1.0);
            }
            PackageOperation::UpgradeAll => {
                let package_ids = self.catalog.upgrade_ids();
                if package_ids.is_empty() {
                    self.status = "No packages to upgrade.".to_owned();
                    progress(self.status(), 1.0);
                    return outcome;
                }
                let total = package_ids.len() as f32;
                for (index, package_id) in package_ids.into_iter().enumerate() {
                    let base = index as f32 / total;
                    let scale = 1.0 / total;
                    let mut step_progress = |status: &str, value: f32| {
                        progress(status, base + value.clamp(0.0, 1.0) * scale);
                    };
                    match self.install_package(client, &package_id, "", "", &mut step_progress) {
                        Ok(PackageInstallOutcome::Installed {
                            package_id,
                            package_type,
                        }) => {
                            outcome.reload_effect_catalog |=
                                matches!(package_type.as_str(), "effect" | "object");
                            outcome.installed.push(package_id);
                        }
                        Ok(PackageInstallOutcome::SelfUpdate { version }) => {
                            outcome.self_update_version = Some(version);
                        }
                        Err(error) => outcome.errors.push(error),
                    }
                }
                self.status = if outcome.errors.is_empty() {
                    "All upgrades complete.".to_owned()
                } else {
                    format!("Upgrades complete with {} error(s).", outcome.errors.len())
                };
                progress(self.status(), 1.0);
            }
        }
        outcome
    }

    fn install_package<C: PackageHttpClient>(
        &mut self,
        client: &C,
        package_id: &str,
        source_repository: &str,
        requested_version: &str,
        progress: &mut dyn FnMut(&str, f32),
    ) -> Result<PackageInstallOutcome, String> {
        if !package_id_is_valid(package_id) {
            return Err("Invalid package ID or type.".to_owned());
        }
        let catalog_entry = self
            .catalog
            .find(package_id, source_repository)
            .ok_or_else(|| format!("Package not found: {package_id}"))?;
        if package_id == "org.aviqtl.app" {
            let version = if requested_version.is_empty() {
                text(catalog_entry.get("latest_version"))
            } else {
                requested_version.to_owned()
            };
            self.status = "AviQtl update available. Restart to apply.".to_owned();
            return Ok(PackageInstallOutcome::SelfUpdate { version });
        }
        let catalog_type = text(catalog_entry.get("type"));
        if !package_type_is_installable(&catalog_type) {
            return Err("Invalid package ID or type.".to_owned());
        }
        let effective_repository = if source_repository.is_empty() {
            text(catalog_entry.get("_primary_repo"))
        } else {
            source_repository.to_owned()
        };
        progress(&format!("Fetching package details: {package_id}"), 0.05);
        let detail = self.fetch_package_detail(client, package_id, &effective_repository)?;
        let selection = select_package_install(&detail, requested_version, &self.app_version);
        match selection.status.as_str() {
            "ok" => {}
            "invalid_type" => return Err("Invalid package ID or type.".to_owned()),
            "no_download" => {
                return Err(format!(
                    "No download URL found for package {package_id} version {}",
                    selection.version
                ));
            }
            "requires_newer_app" => {
                return Err(format!(
                    "Package {package_id} requires AviQtl {} or newer (current: {})",
                    selection.minimum_app_version, self.app_version
                ));
            }
            _ => return Err("Package installation could not be validated.".to_owned()),
        }
        if selection.package_type != catalog_type
            || !package_type_is_installable(&selection.package_type)
        {
            return Err("Package metadata type does not match the catalog.".to_owned());
        }
        progress(&format!("Downloading package: {package_id}"), 0.15);
        let archive = client
            .get(&selection.download_url, MAX_PACKAGE_DOWNLOAD_BYTES)
            .map_err(|error| format!("Download failed: {error}"))?;
        let actual_sha256 = sha256_hex(&archive);
        if !selection.sha256.is_empty() && !actual_sha256.eq_ignore_ascii_case(&selection.sha256) {
            return Err(format!(
                "Checksum mismatch for {package_id}: expected {}, got {actual_sha256}",
                selection.sha256
            ));
        }

        let mut installed =
            read_json_object(&self.root.join("installed.json"), MAX_INSTALLED_STATE_BYTES)
                .unwrap_or_default();
        installed.insert(
            package_id.to_owned(),
            serde_json::json!({
                "version": selection.version,
                "type": selection.package_type,
                "installed_at": installation_timestamp(),
                "installed_from_repo": effective_repository,
                "installed_from_url": selection.download_url,
                "sha256": actual_sha256,
            }),
        );
        let installed_bytes = serde_json::to_vec_pretty(&installed)
            .map_err(|error| format!("Failed to serialize installed package state: {error}"))?;
        progress("Extracting package...", 0.6);
        let installed_path = self.root.join("installed.json");
        deploy_package_archive(
            &self.root,
            package_id,
            &selection.package_type,
            &archive,
            || {
                fs::create_dir_all(&self.root)
                    .map_err(|error| format!("{}: {error}", self.root.display()))?;
                write_atomic(&installed_path, &installed_bytes)
                    .map_err(|error| format!("Failed to save installed package state: {error}"))
            },
        )?;

        progress("Deploying package files...", 0.9);
        self.installed = installed;
        self.catalog
            .set_installed(package_id, Some(&selection.version));
        self.status = format!("Installation complete: {package_id}");
        Ok(PackageInstallOutcome::Installed {
            package_id: package_id.to_owned(),
            package_type: selection.package_type,
        })
    }

    fn fetch_package_detail<C: PackageHttpClient>(
        &mut self,
        client: &C,
        package_id: &str,
        source_repository: &str,
    ) -> Result<Map<String, Value>, String> {
        let cache_key = package_detail_key(package_id, source_repository);
        if let Some(detail) = self.package_details.get(&cache_key) {
            return Ok(detail.clone());
        }
        let catalog_entry = self
            .catalog
            .find(package_id, source_repository)
            .ok_or_else(|| format!("Package not found: {package_id}"))?;
        let metadata_url = text(catalog_entry.get("metadata_url"));
        if !secure_network_url(&metadata_url) {
            return Err(format!(
                "Invalid or insecure metadata URL for package: {package_id}"
            ));
        }
        let body = client
            .get(&metadata_url, MAX_REPOSITORY_METADATA_BYTES)
            .map_err(|error| format!("Failed to fetch package metadata ({package_id}): {error}"))?;
        let expected_sha256 = text(catalog_entry.get("metadata_sha256"));
        if !expected_sha256.is_empty() && !sha256_hex(&body).eq_ignore_ascii_case(&expected_sha256)
        {
            return Err(format!(
                "Metadata checksum mismatch for package {package_id}"
            ));
        }
        let detail = parse_json_object(&body, MAX_REPOSITORY_METADATA_BYTES)
            .and_then(|detail| normalize_package_metadata(&detail))
            .ok_or_else(|| format!("Invalid metadata format for package: {package_id}"))?;
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("{}: {error}", self.root.display()))?;
        let cache_bytes = serde_json::to_vec_pretty(&detail)
            .map_err(|error| format!("Failed to serialize package details: {error}"))?;
        write_atomic(
            &self.root.join(package_detail_cache_name(&metadata_url)),
            &cache_bytes,
        )
        .map_err(|error| format!("Failed to save package details: {error}"))?;
        self.package_details.insert(cache_key, detail.clone());
        Ok(detail)
    }

    fn remove_installed_package(&mut self, package_id: &str) -> Result<String, String> {
        if package_id == "org.aviqtl.app" || !package_id_is_valid(package_id) {
            return Err("Invalid package ID.".to_owned());
        }
        let current =
            read_json_object(&self.root.join("installed.json"), MAX_INSTALLED_STATE_BYTES)
                .unwrap_or_default();
        let installed_package = current
            .get(package_id)
            .and_then(Value::as_object)
            .ok_or_else(|| {
                "Cannot remove package because its installed type is missing or invalid.".to_owned()
            })?;
        let package_type = text(installed_package.get("type"));
        if !package_type_is_installable(&package_type) {
            return Err(
                "Cannot remove package because its installed type is missing or invalid."
                    .to_owned(),
            );
        }
        let mut updated = current;
        updated.remove(package_id);
        let installed_bytes = serde_json::to_vec_pretty(&updated)
            .map_err(|error| format!("Failed to serialize installed package state: {error}"))?;
        let installed_path = self.root.join("installed.json");
        remove_package_transaction(&self.root, package_id, &package_type, || {
            fs::create_dir_all(&self.root)
                .map_err(|error| format!("{}: {error}", self.root.display()))?;
            write_atomic(&installed_path, &installed_bytes)
                .map_err(|error| format!("Failed to save installed package state: {error}"))
        })?;
        self.installed = updated;
        self.catalog.set_installed(package_id, None);
        self.status = format!("Removal complete: {package_id}");
        Ok(package_type)
    }

    pub fn packages(&self, section: PackageSection, query: &str) -> Vec<PackageListItem> {
        let query = query.trim().to_lowercase();
        let mut packages = self
            .catalog
            .packages_by_type(section.package_type())
            .into_iter()
            .filter_map(|package| package.as_object().cloned())
            .map(|package| package_item(&package))
            .filter(|package| package_matches(package, &query))
            .collect::<Vec<_>>();
        if matches!(section, PackageSection::Mod | PackageSection::Installed) {
            let present = packages
                .iter()
                .map(|package| package.id.clone())
                .collect::<BTreeSet<_>>();
            packages.extend(
                local_file_plugins(&self.root)
                    .into_iter()
                    .filter(|package| !present.contains(&package.id))
                    .filter(|package| package_matches(package, &query)),
            );
        }
        packages
    }

    pub fn reload_cached_packages(&mut self) {
        self.catalog.clear();
        self.installed =
            read_json_object(&self.root.join("installed.json"), MAX_INSTALLED_STATE_BYTES)
                .unwrap_or_default();
        let mut cache_files = fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("catalog_") && name.ends_with(".json"))
            })
            .collect::<Vec<_>>();
        cache_files.sort();
        let mut loaded = 0usize;
        for path in cache_files {
            let Some(document) = read_json_object(&path, MAX_REPOSITORY_METADATA_BYTES) else {
                continue;
            };
            let repository_url = text(document.get("_repo_url"));
            let repository = Map::from_iter([("url".to_owned(), Value::String(repository_url))]);
            let packages = document
                .get("packages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            self.catalog.merge(
                &packages,
                &repository,
                &self.repositories,
                &self.installed,
                &self.language,
                &self.app_version,
            );
            loaded += 1;
        }
        self.status = if loaded == 0 {
            "Idle".to_owned()
        } else {
            "Packages loaded from cache (Press Sync to check for updates)".to_owned()
        };
    }

    pub fn add_repository(
        &mut self,
        settings: &mut SettingsStore,
        url: &str,
    ) -> Result<bool, String> {
        if !secure_network_url(url) {
            return Err(format!("Repository URL must use HTTPS: {url}"));
        }
        self.mutate_repositories(
            settings,
            url,
            PackageRepositoryOperation::Add {
                enabled: true,
                priority: 10,
            },
        )
    }

    pub fn remove_repository(
        &mut self,
        settings: &mut SettingsStore,
        url: &str,
    ) -> Result<bool, String> {
        self.mutate_repositories(settings, url, PackageRepositoryOperation::Remove)
    }

    pub fn set_repository_enabled(
        &mut self,
        settings: &mut SettingsStore,
        url: &str,
        enabled: bool,
    ) -> Result<bool, String> {
        self.mutate_repositories(
            settings,
            url,
            PackageRepositoryOperation::SetEnabled(enabled),
        )
    }

    fn mutate_repositories(
        &mut self,
        settings: &mut SettingsStore,
        url: &str,
        operation: PackageRepositoryOperation,
    ) -> Result<bool, String> {
        let Some((repositories, changed)) =
            mutate_package_repositories(&self.repositories, url, operation)
        else {
            return Err("Invalid repository operation".to_owned());
        };
        if !changed {
            return Ok(false);
        }
        let mut replacement = settings.snapshot();
        replacement.insert(
            "packageRepositories".to_owned(),
            Value::Array(repositories.clone()),
        );
        settings.apply(replacement)?;
        self.repositories = repositories;
        Ok(true)
    }

    fn fetch_repository<C: PackageHttpClient>(
        &mut self,
        client: &C,
        repository_url: &str,
        repository_info: &mut Map<String, Value>,
    ) -> Result<(), String> {
        let metadata_url = repository_metadata_url(repository_url)
            .ok_or_else(|| format!("Repository URL must use HTTPS: {repository_url}"))?;
        let metadata_bytes = client
            .get(metadata_url.as_str(), MAX_REPOSITORY_METADATA_BYTES)
            .map_err(|error| format!("Failed to fetch repository {repository_url}: {error}"))?;
        let metadata = parse_json_object(&metadata_bytes, MAX_REPOSITORY_METADATA_BYTES)
            .ok_or_else(|| format!("Repository metadata is invalid: {repository_url}"))?;
        let repository_name = text(metadata.get("repo_name"));
        if !repository_name.is_empty() {
            repository_info.insert("name".to_owned(), Value::String(repository_name));
        }
        let catalog_reference = text(metadata.get("catalog_url"));
        let catalog_bytes = if catalog_reference.is_empty() {
            metadata_bytes
        } else {
            let catalog_url = metadata_url
                .join(&catalog_reference)
                .ok()
                .filter(|url| secure_network_url(url.as_str()))
                .ok_or_else(|| format!("Catalog URL must use HTTPS: {catalog_reference}"))?;
            client
                .get(catalog_url.as_str(), MAX_REPOSITORY_METADATA_BYTES)
                .map_err(|error| {
                    format!("Failed to fetch repository catalog {catalog_url}: {error}")
                })?
        };
        let catalog_document = parse_json_object(&catalog_bytes, MAX_REPOSITORY_METADATA_BYTES)
            .ok_or_else(|| format!("Repository catalog is invalid: {repository_url}"))?;
        let packages = catalog_document
            .get("packages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.catalog.merge(
            &packages,
            repository_info,
            &self.repositories,
            &self.installed,
            &self.language,
            &self.app_version,
        );

        let cache_document = serde_json::json!({
            "_repo_url": repository_url,
            "packages": packages,
        });
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("{}: {error}", self.root.display()))?;
        let cache_bytes = serde_json::to_vec_pretty(&cache_document)
            .map_err(|error| format!("Failed to serialize repository cache: {error}"))?;
        write_atomic(
            &self.root.join(repository_cache_name(repository_url)),
            &cache_bytes,
        )
        .map_err(|error| format!("Failed to save repository cache: {error}"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZipEntry {
    path: PathBuf,
    compression_method: u16,
    crc32: u32,
    compressed_start: usize,
    compressed_size: usize,
    uncompressed_size: u64,
    directory: bool,
}

fn deploy_package_archive<F>(
    package_root: &Path,
    package_id: &str,
    package_type: &str,
    archive: &[u8],
    commit_state: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    if !package_id_is_valid(package_id) || !package_type_is_installable(package_type) {
        return Err("Invalid package ID or type.".to_owned());
    }
    let deploy_base = package_deploy_directory(package_root, package_type)
        .ok_or_else(|| "Invalid package ID or type.".to_owned())?;
    fs::create_dir_all(&deploy_base)
        .map_err(|error| format!("{}: {error}", deploy_base.display()))?;
    let workspace = transaction_workspace(&deploy_base);
    let extraction_root = create_unique_directory(&workspace, "staging")?;
    let extraction_result = extract_zip_archive(archive, &extraction_root);
    if let Err(error) = extraction_result {
        let _ = fs::remove_dir_all(&extraction_root);
        let _ = fs::remove_dir(&workspace);
        return Err(error);
    }
    let source = deployment_source(&extraction_root)?;
    let target = deploy_base.join(package_id);
    let backup = workspace.join(format!(".backup_{package_id}"));
    if path_exists(&backup) {
        remove_path(&backup).map_err(|error| {
            format!(
                "Could not remove stale package backup {}: {error}",
                backup.display()
            )
        })?;
    }
    let had_existing = path_exists(&target);
    if had_existing {
        fs::rename(&target, &backup).map_err(|error| {
            format!(
                "Could not create package backup {}: {error}",
                target.display()
            )
        })?;
    }
    if let Err(error) = fs::rename(&source, &target) {
        let restored = !had_existing || fs::rename(&backup, &target).is_ok();
        let _ = fs::remove_dir_all(&extraction_root);
        let _ = fs::remove_dir(&workspace);
        return if restored {
            Err(format!(
                "Failed to deploy package; the previous installation was restored: {error}"
            ))
        } else {
            Err(format!(
                "Package deployment failed and automatic rollback was incomplete; the backup was preserved at {}",
                backup.display()
            ))
        };
    }
    if let Err(error) = commit_state() {
        let reverted = remove_path(&target).is_ok();
        let restored = !had_existing || (reverted && fs::rename(&backup, &target).is_ok());
        let _ = fs::remove_dir_all(&extraction_root);
        let _ = fs::remove_dir(&workspace);
        return if restored {
            Err(format!(
                "Failed to deploy package; the previous installation was restored: {error}"
            ))
        } else {
            Err(format!(
                "Package deployment failed and automatic rollback was incomplete; the backup was preserved at {}",
                backup.display()
            ))
        };
    }
    if had_existing && let Err(error) = remove_path(&backup) {
        eprintln!(
            "Package deployment succeeded but backup cleanup failed ({}): {error}",
            backup.display()
        );
    }
    let _ = fs::remove_dir_all(&extraction_root);
    let _ = fs::remove_dir(&workspace);
    Ok(())
}

fn remove_package_transaction<F>(
    package_root: &Path,
    package_id: &str,
    package_type: &str,
    commit_state: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    if !package_id_is_valid(package_id) || !package_type_is_installable(package_type) {
        return Err("Invalid package ID or type.".to_owned());
    }
    let deploy_base = package_deploy_directory(package_root, package_type)
        .ok_or_else(|| "Invalid package ID or type.".to_owned())?;
    let workspace = transaction_workspace(&deploy_base);
    fs::create_dir_all(&workspace).map_err(|error| format!("{}: {error}", workspace.display()))?;
    let target = deploy_base.join(package_id);
    let backup = workspace.join(format!(".remove_backup_{package_id}"));
    if path_exists(&backup) {
        remove_path(&backup).map_err(|error| {
            format!(
                "Could not remove stale package backup {}: {error}",
                backup.display()
            )
        })?;
    }
    let had_existing = path_exists(&target);
    if had_existing {
        fs::rename(&target, &backup).map_err(|error| {
            format!(
                "Could not create package removal backup {}: {error}",
                target.display()
            )
        })?;
    }
    if let Err(error) = commit_state() {
        let restored = !had_existing || fs::rename(&backup, &target).is_ok();
        let _ = fs::remove_dir(&workspace);
        return if restored {
            Err(format!(
                "Failed to remove package; the installed state and files were restored: {error}"
            ))
        } else {
            Err(format!(
                "Package removal failed and automatic rollback was incomplete; the backup was preserved at {}",
                backup.display()
            ))
        };
    }
    if had_existing && let Err(error) = remove_path(&backup) {
        eprintln!(
            "Package removal succeeded but backup cleanup failed ({}): {error}",
            backup.display()
        );
    }
    let _ = fs::remove_dir(&workspace);
    Ok(())
}

fn package_deploy_directory(package_root: &Path, package_type: &str) -> Option<PathBuf> {
    let application_root = package_root.parent()?;
    match package_type {
        "mod" => Some(application_root.join("plugins")),
        "effect" => Some(application_root.join("effects")),
        "object" => Some(application_root.join("objects")),
        _ => None,
    }
}

fn transaction_workspace(deploy_base: &Path) -> PathBuf {
    let directory_name = deploy_base
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("packages");
    deploy_base
        .parent()
        .unwrap_or(deploy_base)
        .join(format!(".{directory_name}-package-deployment"))
}

fn create_unique_directory(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for sequence in 0..128_u32 {
        let path = parent.join(format!(
            ".{prefix}_{}_{}_{}",
            std::process::id(),
            seed,
            sequence
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    Err(format!(
        "Failed to allocate transaction directory under {}",
        parent.display()
    ))
}

fn deployment_source(extraction_root: &Path) -> Result<PathBuf, String> {
    let mut entries = fs::read_dir(extraction_root)
        .map_err(|error| format!("{}: {error}", extraction_root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{}: {error}", extraction_root.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() == 1
        && entries[0]
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
    {
        Ok(entries.remove(0).path())
    } else {
        Ok(extraction_root.to_path_buf())
    }
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn extract_zip_archive(archive: &[u8], destination: &Path) -> Result<(), String> {
    let entries = parse_zip_entries(archive)?;
    fs::create_dir_all(destination)
        .map_err(|error| format!("{}: {error}", destination.display()))?;
    for entry in entries {
        let path = destination.join(&entry.path);
        if entry.directory {
            fs::create_dir_all(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let compressed_end = entry
            .compressed_start
            .checked_add(entry.compressed_size)
            .ok_or_else(|| "Package archive entry is too large.".to_owned())?;
        let compressed = archive
            .get(entry.compressed_start..compressed_end)
            .ok_or_else(|| "Package archive entry is truncated.".to_owned())?;
        let mut output =
            File::create(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let (written, crc32, consumed) = match entry.compression_method {
            0 => {
                let mut reader = Cursor::new(compressed);
                let (written, crc32) =
                    copy_zip_entry(&mut reader, &mut output, entry.uncompressed_size)?;
                (written, crc32, reader.position())
            }
            8 => {
                let mut reader = DeflateDecoder::new(Cursor::new(compressed));
                let (written, crc32) =
                    copy_zip_entry(&mut reader, &mut output, entry.uncompressed_size)?;
                (written, crc32, reader.total_in())
            }
            method => return Err(format!("Unsupported ZIP compression method: {method}")),
        };
        if written != entry.uncompressed_size
            || consumed != entry.compressed_size as u64
            || crc32 != entry.crc32
        {
            return Err(format!(
                "Package archive entry failed integrity validation: {}",
                entry.path.display()
            ));
        }
        output
            .sync_all()
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

fn copy_zip_entry<R: Read>(
    reader: &mut R,
    output: &mut File,
    expected_size: u64,
) -> Result<(u64, u32), String> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut written = 0_u64;
    let mut crc32 = Crc32::new();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        written = written
            .checked_add(count as u64)
            .ok_or_else(|| "Package archive entry is too large.".to_owned())?;
        if written > expected_size || written > MAX_PACKAGE_EXTRACTED_BYTES {
            return Err("Package archive extracted data exceeds the allowed size.".to_owned());
        }
        crc32.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
    }
    Ok((written, crc32.finalize()))
}

fn parse_zip_entries(archive: &[u8]) -> Result<Vec<ZipEntry>, String> {
    const EOCD_SIGNATURE: u32 = 0x0605_4b50;
    const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
    const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
    let minimum_eocd = 22_usize;
    if archive.len() < minimum_eocd {
        return Err("Package archive is not a readable ZIP file.".to_owned());
    }
    let search_start = archive
        .len()
        .saturating_sub(minimum_eocd + u16::MAX as usize);
    let mut eocd = None;
    for offset in (search_start..=archive.len() - minimum_eocd).rev() {
        if read_u32(archive, offset) == Some(EOCD_SIGNATURE) {
            let comment_length = read_u16(archive, offset + 20).unwrap_or_default() as usize;
            if offset + minimum_eocd + comment_length == archive.len() {
                eocd = Some(offset);
                break;
            }
        }
    }
    let eocd = eocd.ok_or_else(|| "Package archive is not a readable ZIP file.".to_owned())?;
    let disk_number = read_u16(archive, eocd + 4).unwrap_or(u16::MAX);
    let central_disk = read_u16(archive, eocd + 6).unwrap_or(u16::MAX);
    let disk_entries = read_u16(archive, eocd + 8).unwrap_or(u16::MAX);
    let total_entries = read_u16(archive, eocd + 10).unwrap_or(u16::MAX);
    let central_size = read_u32(archive, eocd + 12).unwrap_or(u32::MAX);
    let central_offset = read_u32(archive, eocd + 16).unwrap_or(u32::MAX);
    if disk_number != 0
        || central_disk != 0
        || disk_entries != total_entries
        || total_entries == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX
        || total_entries as usize > MAX_PACKAGE_ARCHIVE_ENTRIES
    {
        return Err("Unsupported or oversized ZIP archive.".to_owned());
    }
    let central_start = central_offset as usize;
    let central_end = central_start
        .checked_add(central_size as usize)
        .filter(|end| *end <= eocd)
        .ok_or_else(|| "Package archive central directory is invalid.".to_owned())?;
    let mut cursor = central_start;
    let mut extracted_bytes = 0_u64;
    let mut paths = BTreeSet::new();
    let mut entries = Vec::with_capacity(total_entries as usize);
    for _ in 0..total_entries {
        if read_u32(archive, cursor) != Some(CENTRAL_SIGNATURE) {
            return Err("Package archive central directory is invalid.".to_owned());
        }
        let flags = required_u16(archive, cursor + 8)?;
        let method = required_u16(archive, cursor + 10)?;
        let crc32 = required_u32(archive, cursor + 16)?;
        let compressed_size = required_u32(archive, cursor + 20)?;
        let uncompressed_size = required_u32(archive, cursor + 24)?;
        let name_length = required_u16(archive, cursor + 28)? as usize;
        let extra_length = required_u16(archive, cursor + 30)? as usize;
        let comment_length = required_u16(archive, cursor + 32)? as usize;
        let disk_start = required_u16(archive, cursor + 34)?;
        let external_attributes = required_u32(archive, cursor + 38)?;
        let local_offset = required_u32(archive, cursor + 42)?;
        if flags & 1 != 0
            || disk_start != 0
            || compressed_size == u32::MAX
            || uncompressed_size == u32::MAX
            || local_offset == u32::MAX
            || !matches!(method, 0 | 8)
        {
            return Err("Unsupported ZIP archive entry.".to_owned());
        }
        let header_end = cursor
            .checked_add(46)
            .ok_or_else(|| "Package archive central directory is invalid.".to_owned())?;
        let name_end = header_end
            .checked_add(name_length)
            .ok_or_else(|| "Package archive central directory is invalid.".to_owned())?;
        let entry_end = name_end
            .checked_add(extra_length)
            .and_then(|end| end.checked_add(comment_length))
            .filter(|end| *end <= central_end)
            .ok_or_else(|| "Package archive central directory is invalid.".to_owned())?;
        let name_bytes = archive
            .get(header_end..name_end)
            .ok_or_else(|| "Package archive entry name is truncated.".to_owned())?;
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| "Package archive entry names must be UTF-8.".to_owned())?;
        if !package_archive_path_is_safe(name) {
            return Err(format!("Unsafe package archive entry: {name}"));
        }
        let path = normalize_archive_path(name)
            .ok_or_else(|| format!("Unsafe package archive entry: {name}"))?;
        if !paths.insert(path.clone()) {
            return Err(format!(
                "Duplicate package archive entry: {}",
                path.display()
            ));
        }
        let unix_mode = external_attributes >> 16;
        let file_type = unix_mode & 0o170000;
        if file_type == 0o120000 {
            return Err(format!(
                "Symbolic links are not allowed in packages: {name}"
            ));
        }
        let directory = name.ends_with('/') || file_type == 0o040000;
        if directory && (compressed_size != 0 || uncompressed_size != 0) {
            return Err(format!("Invalid package directory entry: {name}"));
        }
        extracted_bytes = extracted_bytes
            .checked_add(uncompressed_size as u64)
            .filter(|bytes| *bytes <= MAX_PACKAGE_EXTRACTED_BYTES)
            .ok_or_else(|| "Package archive extracted data exceeds the allowed size.".to_owned())?;

        let local_offset = local_offset as usize;
        if read_u32(archive, local_offset) != Some(LOCAL_SIGNATURE) {
            return Err("Package archive local header is invalid.".to_owned());
        }
        let local_flags = required_u16(archive, local_offset + 6)?;
        let local_method = required_u16(archive, local_offset + 8)?;
        let local_name_length = required_u16(archive, local_offset + 26)? as usize;
        let local_extra_length = required_u16(archive, local_offset + 28)? as usize;
        if local_flags != flags || local_method != method || local_name_length != name_length {
            return Err(
                "Package archive local header does not match its catalog entry.".to_owned(),
            );
        }
        let local_name_start = local_offset
            .checked_add(30)
            .ok_or_else(|| "Package archive local header is invalid.".to_owned())?;
        let local_name_end = local_name_start
            .checked_add(local_name_length)
            .ok_or_else(|| "Package archive local header is invalid.".to_owned())?;
        if archive.get(local_name_start..local_name_end) != Some(name_bytes) {
            return Err(
                "Package archive local filename does not match its catalog entry.".to_owned(),
            );
        }
        let compressed_start = local_name_end
            .checked_add(local_extra_length)
            .filter(|start| *start <= central_start)
            .ok_or_else(|| "Package archive local header is invalid.".to_owned())?;
        let compressed_size = compressed_size as usize;
        compressed_start
            .checked_add(compressed_size)
            .filter(|end| *end <= central_start)
            .ok_or_else(|| "Package archive entry data is truncated.".to_owned())?;
        entries.push(ZipEntry {
            path,
            compression_method: method,
            crc32,
            compressed_start,
            compressed_size,
            uncompressed_size: uncompressed_size as u64,
            directory,
        });
        cursor = entry_end;
    }
    if cursor != central_end {
        return Err("Package archive central directory has trailing data.".to_owned());
    }
    Ok(entries)
}

fn normalize_archive_path(value: &str) -> Option<PathBuf> {
    let mut components = Vec::new();
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            component => components.push(component),
        }
    }
    (!components.is_empty()).then(|| components.into_iter().collect())
}

fn required_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    read_u16(bytes, offset).ok_or_else(|| "Package archive is truncated.".to_owned())
}

fn required_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    read_u32(bytes, offset).ok_or_else(|| "Package archive is truncated.".to_owned())
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn package_detail_key(package_id: &str, source_repository: &str) -> String {
    if source_repository.is_empty() {
        package_id.to_owned()
    } else {
        format!("{source_repository}|{package_id}")
    }
}

fn package_detail_cache_name(metadata_url: &str) -> String {
    format!("detail_{}.json", sha256_hex(metadata_url.as_bytes()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn installation_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day % 3_600 / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn package_item(package: &Map<String, Value>) -> PackageListItem {
    let installed_version = text(package.get("installed_version"));
    let latest_version = text(package.get("latest_version"));
    PackageListItem {
        id: text(package.get("id")),
        package_type: text(package.get("type")),
        display_name: text(package.get("display_name")),
        description: text(package.get("description")),
        author: text(package.get("author")),
        version: text(package.get("version")),
        installed_version: installed_version.clone(),
        latest_version: latest_version.clone(),
        source_repository: text(package.get("_primary_repo")),
        local_file_plugin: package
            .get("local_file_plugin")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_update: !installed_version.is_empty()
            && !latest_version.is_empty()
            && installed_version != latest_version,
    }
}

fn local_file_plugins(root: &Path) -> Vec<PackageListItem> {
    let Some(plugins_root) = root.parent().map(|parent| parent.join("plugins")) else {
        return Vec::new();
    };
    let mut files = fs::read_dir(plugins_root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("lua"))
        .filter(|path| {
            fs::metadata(path).is_ok_and(|metadata| {
                metadata.is_file() && metadata.len() <= MAX_PLUGIN_SCRIPT_BYTES
            })
        })
        .collect::<Vec<_>>();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let display_name = path.file_stem()?.to_str()?.to_owned();
            Some(PackageListItem {
                id: format!("file:{name}"),
                package_type: "mod".to_owned(),
                display_name,
                description: String::new(),
                author: String::new(),
                version: "file".to_owned(),
                installed_version: "file".to_owned(),
                latest_version: String::new(),
                source_repository: String::new(),
                local_file_plugin: true,
                has_update: false,
            })
        })
        .collect()
}

fn package_matches(package: &PackageListItem, query: &str) -> bool {
    query.is_empty()
        || package.display_name.to_lowercase().contains(query)
        || package.id.to_lowercase().contains(query)
}

fn configured_repositories(settings: &SettingsStore) -> Vec<Value> {
    settings
        .value("packageRepositories")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn secure_network_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn repository_metadata_url(value: &str) -> Option<Url> {
    let mut url = Url::parse(value).ok()?;
    if !secure_network_url(url.as_str()) {
        return None;
    }
    if !url.path().ends_with("/repo.json") {
        let path = if url.path().ends_with('/') {
            format!("{}repo.json", url.path())
        } else {
            format!("{}/repo.json", url.path())
        };
        url.set_path(&path);
    }
    Some(url)
}

fn repository_cache_name(repository_url: &str) -> String {
    let digest = Sha256::digest(repository_url.as_bytes());
    let digest = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("catalog_{digest}.json")
}

fn read_json_object(path: &Path, maximum_bytes: u64) -> Option<Map<String, Value>> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > maximum_bytes {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.len() as u64 > maximum_bytes {
        return None;
    }
    parse_json_object(&bytes, maximum_bytes)
}

fn parse_json_object(bytes: &[u8], maximum_bytes: u64) -> Option<Map<String, Value>> {
    if bytes.len() as u64 > maximum_bytes {
        return None;
    }
    serde_json::from_slice::<Value>(bytes)
        .ok()?
        .as_object()
        .cloned()
}

fn default_package_root() -> PathBuf {
    package_paths().package_root
}

fn system_language() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find_map(|locale| {
            let language = locale
                .split(['_', '.', '-'])
                .next()
                .unwrap_or_default()
                .to_lowercase();
            (language.len() == 2).then_some(language)
        })
        .unwrap_or_else(|| "en".to_owned())
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temporary_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "aviqtl-package-manager-{}-{nanos}-{}",
                std::process::id(),
                PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ))
            .join("repos")
    }

    #[test]
    fn package_state_uses_the_user_data_root() {
        let paths = package_paths();
        assert_eq!(
            paths.package_root,
            crate::settings::application_data_root().join("repos")
        );
        assert!(
            paths
                .effect_roots
                .contains(&crate::settings::application_data_root().join("effects"))
        );
        assert!(
            paths
                .object_roots
                .contains(&crate::settings::application_data_root().join("objects"))
        );
    }

    struct FakeHttpClient {
        responses: BTreeMap<String, Result<Vec<u8>, String>>,
    }

    impl PackageHttpClient for FakeHttpClient {
        fn get(&self, url: &str, maximum_bytes: u64) -> Result<Vec<u8>, String> {
            let bytes = self
                .responses
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err(format!("unexpected URL: {url}")))?;
            if bytes.len() as u64 > maximum_bytes {
                return Err("fixture exceeds limit".to_owned());
            }
            Ok(bytes)
        }
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn stored_zip(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        struct CentralEntry {
            name: Vec<u8>,
            crc32: u32,
            size: u32,
            offset: u32,
            external_attributes: u32,
        }

        let mut bytes = Vec::new();
        let mut central = Vec::new();
        for (name, data, external_attributes) in entries {
            let name = name.as_bytes();
            let crc32 = crc32fast::hash(data);
            let size = u32::try_from(data.len()).expect("fixture data fits u32");
            let offset = u32::try_from(bytes.len()).expect("fixture offset fits u32");
            push_u32(&mut bytes, 0x0403_4b50);
            push_u16(&mut bytes, 20);
            push_u16(&mut bytes, 0x0800);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, crc32);
            push_u32(&mut bytes, size);
            push_u32(&mut bytes, size);
            push_u16(
                &mut bytes,
                u16::try_from(name.len()).expect("fixture name fits u16"),
            );
            push_u16(&mut bytes, 0);
            bytes.extend_from_slice(name);
            bytes.extend_from_slice(data);
            central.push(CentralEntry {
                name: name.to_vec(),
                crc32,
                size,
                offset,
                external_attributes: *external_attributes,
            });
        }
        let central_offset = u32::try_from(bytes.len()).expect("fixture offset fits u32");
        for entry in &central {
            push_u32(&mut bytes, 0x0201_4b50);
            push_u16(&mut bytes, 0x0314);
            push_u16(&mut bytes, 20);
            push_u16(&mut bytes, 0x0800);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, entry.crc32);
            push_u32(&mut bytes, entry.size);
            push_u32(&mut bytes, entry.size);
            push_u16(
                &mut bytes,
                u16::try_from(entry.name.len()).expect("fixture name fits u16"),
            );
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, entry.external_attributes);
            push_u32(&mut bytes, entry.offset);
            bytes.extend_from_slice(&entry.name);
        }
        let central_size = u32::try_from(bytes.len())
            .expect("fixture size fits u32")
            .saturating_sub(central_offset);
        push_u32(&mut bytes, 0x0605_4b50);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(
            &mut bytes,
            u16::try_from(central.len()).expect("fixture entry count fits u16"),
        );
        push_u16(
            &mut bytes,
            u16::try_from(central.len()).expect("fixture entry count fits u16"),
        );
        push_u32(&mut bytes, central_size);
        push_u32(&mut bytes, central_offset);
        push_u16(&mut bytes, 0);
        bytes
    }

    fn deflated_zip(name: &str, data: &[u8], external_attributes: u32) -> Vec<u8> {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).expect("fixture deflates");
        let compressed = encoder.finish().expect("fixture deflate finishes");
        let name = name.as_bytes();
        let crc32 = crc32fast::hash(data);
        let compressed_size = u32::try_from(compressed.len()).expect("fixture data fits u32");
        let uncompressed_size = u32::try_from(data.len()).expect("fixture data fits u32");
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0x0403_4b50);
        push_u16(&mut bytes, 20);
        push_u16(&mut bytes, 0x0800);
        push_u16(&mut bytes, 8);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, crc32);
        push_u32(&mut bytes, compressed_size);
        push_u32(&mut bytes, uncompressed_size);
        push_u16(
            &mut bytes,
            u16::try_from(name.len()).expect("fixture name fits u16"),
        );
        push_u16(&mut bytes, 0);
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&compressed);
        let central_offset = u32::try_from(bytes.len()).expect("fixture offset fits u32");
        push_u32(&mut bytes, 0x0201_4b50);
        push_u16(&mut bytes, 0x0314);
        push_u16(&mut bytes, 20);
        push_u16(&mut bytes, 0x0800);
        push_u16(&mut bytes, 8);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, crc32);
        push_u32(&mut bytes, compressed_size);
        push_u32(&mut bytes, uncompressed_size);
        push_u16(
            &mut bytes,
            u16::try_from(name.len()).expect("fixture name fits u16"),
        );
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, external_attributes);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(name);
        let central_size = u32::try_from(bytes.len())
            .expect("fixture size fits u32")
            .saturating_sub(central_offset);
        push_u32(&mut bytes, 0x0605_4b50);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, central_size);
        push_u32(&mut bytes, central_offset);
        push_u16(&mut bytes, 0);
        bytes
    }

    #[test]
    fn cached_catalog_projects_qt_tabs_search_updates_and_local_mods() {
        let root = temporary_root();
        fs::create_dir_all(root.parent().expect("root has parent").join("plugins"))
            .expect("plugin directory creates");
        fs::create_dir_all(&root).expect("cache directory creates");
        fs::write(
            root.join("installed.json"),
            serde_json::to_vec(&json!({"effect.blur":{"version":"1.0.0"}}))
                .expect("installed state serializes"),
        )
        .expect("installed state writes");
        fs::write(
            root.join("catalog_fixture.json"),
            serde_json::to_vec(&json!({
                "_repo_url":"https://example.invalid",
                "packages":[
                    {"id":"effect.blur","type":"effect","version":"2.0.0","display_name":{"en":"Blur"},"short_description":{"en":"Soft blur"},"author":"AviQtl"},
                    {"id":"mod.tools","type":"mod","version":"1.0.0","display_name":{"en":"Tools"}}
                ]
            }))
            .expect("catalog serializes"),
        )
        .expect("catalog writes");
        fs::write(
            root.parent()
                .expect("root has parent")
                .join("plugins/local.lua"),
            "return {}",
        )
        .expect("local plugin writes");
        let repositories = vec![json!({
            "url":"https://example.invalid",
            "name":"Example",
            "enabled":true,
            "priority":10
        })];

        let model = PackageManagerModel::load_from(
            root.clone(),
            repositories,
            "en".to_owned(),
            "0.6.2".to_owned(),
        );
        let effects = model.packages(PackageSection::Effect, "BLU");
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].display_name, "Blur");
        assert!(effects[0].has_update);
        assert!(model.has_updates());
        assert_eq!(model.packages(PackageSection::Installed, "").len(), 2);
        assert!(
            model
                .packages(PackageSection::Mod, "local")
                .iter()
                .any(|package| package.id == "file:local.lua")
        );
        assert_eq!(model.repositories()[0].name, "Example");
        assert_eq!(
            model.status(),
            "Packages loaded from cache (Press Sync to check for updates)"
        );

        fs::remove_dir_all(root.parent().expect("root has parent"))
            .expect("temporary package root removes");
    }

    #[test]
    fn repository_urls_require_https_without_credentials() {
        assert!(secure_network_url("https://example.invalid/packages"));
        assert!(!secure_network_url("http://example.invalid/packages"));
        assert!(!secure_network_url("https://user@example.invalid/packages"));
        assert!(!secure_network_url("not a URL"));
    }

    #[test]
    fn repository_sync_keeps_partial_success_relative_catalogs_and_cache_provenance() {
        let root = temporary_root();
        let repositories = vec![
            json!({"url":"https://good.invalid","enabled":true,"priority":1}),
            json!({"url":"https://bad.invalid/repo.json","enabled":true,"priority":2}),
        ];
        let client = FakeHttpClient {
            responses: BTreeMap::from([
                (
                    "https://good.invalid/repo.json".to_owned(),
                    Ok(serde_json::to_vec(&json!({
                        "repo_name":"Good",
                        "catalog_url":"catalog.json"
                    }))
                    .expect("metadata serializes")),
                ),
                (
                    "https://good.invalid/catalog.json".to_owned(),
                    Ok(serde_json::to_vec(&json!({
                        "packages":[
                            {"id":"effect.blur","type":"effect","version":"1.0.0","display_name":{"en":"Blur"}},
                            {"id":"org.aviqtl.app","type":"application","version":"0.7.0","display_name":{"en":"AviQtl Plus"}}
                        ]
                    }))
                    .expect("catalog serializes")),
                ),
                (
                    "https://bad.invalid/repo.json".to_owned(),
                    Err("offline".to_owned()),
                ),
            ]),
        };
        let mut model = PackageManagerModel::load_from(
            root.clone(),
            repositories,
            "en".to_owned(),
            "0.6.2".to_owned(),
        );

        let outcome = model.refresh_repositories(&client);
        assert_eq!(outcome.repositories_synced, 1);
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(
            model.packages(PackageSection::Effect, "")[0].id,
            "effect.blur"
        );
        let application = &model.packages(PackageSection::Application, "")[0];
        assert_eq!(application.installed_version, "0.6.2");
        assert!(application.has_update);
        assert!(
            root.join(repository_cache_name("https://good.invalid"))
                .is_file()
        );

        fs::remove_dir_all(root.parent().expect("root has parent"))
            .expect("temporary package root removes");
    }

    #[test]
    fn repository_mutations_persist_through_the_shared_settings_store() {
        let root = temporary_root();
        let base = root.parent().expect("root has parent").to_path_buf();
        fs::create_dir_all(&base).expect("temporary package root creates");
        let settings_path = base.join("settings.json");
        let (mut settings, _) = SettingsStore::load_from(settings_path.clone(), Map::new());
        let mut model = PackageManagerModel::load_from(
            root,
            configured_repositories(&settings),
            "en".to_owned(),
            "0.6.2".to_owned(),
        );

        assert!(
            model
                .add_repository(&mut settings, "https://example.invalid/packages")
                .expect("repository adds")
        );
        assert!(
            !model
                .add_repository(&mut settings, "https://example.invalid/packages")
                .expect("duplicate repository is ignored")
        );
        assert!(
            model
                .set_repository_enabled(&mut settings, "https://example.invalid/packages", false,)
                .expect("repository disables")
        );
        assert!(
            model
                .remove_repository(&mut settings, "https://example.invalid/packages")
                .expect("repository removes")
        );

        let persisted = serde_json::from_slice::<Value>(
            &fs::read(settings_path).expect("settings remain readable"),
        )
        .expect("settings remain valid JSON");
        assert!(
            persisted["packageRepositories"]
                .as_array()
                .is_some_and(|repositories| repositories
                    .iter()
                    .all(|repository| { repository["url"] != "https://example.invalid/packages" }))
        );

        fs::remove_dir_all(base).expect("temporary package root removes");
    }

    #[test]
    fn package_install_and_remove_use_verified_atomic_deployment() {
        let root = temporary_root();
        fs::create_dir_all(&root).expect("package root creates");
        let archive = deflated_zip(
            "wrapper/main.json",
            br#"{"id":"effect.demo"}"#,
            0o100644_u32 << 16,
        );
        let metadata = serde_json::to_vec(&json!({
            "type":"effect",
            "version":"2.0.0",
            "download_url":"https://packages.invalid/effect.demo.zip",
            "download_sha256":sha256_hex(&archive)
        }))
        .expect("metadata serializes");
        let repository = "https://packages.invalid";
        fs::write(
            root.join("catalog_fixture.json"),
            serde_json::to_vec(&json!({
                "_repo_url":repository,
                "packages":[{
                    "id":"effect.demo",
                    "type":"effect",
                    "version":"2.0.0",
                    "display_name":{"en":"Demo"},
                    "metadata_url":"https://packages.invalid/effect.demo.json",
                    "metadata_sha256":sha256_hex(&metadata)
                }]
            }))
            .expect("catalog serializes"),
        )
        .expect("catalog writes");
        let client = FakeHttpClient {
            responses: BTreeMap::from([
                (
                    "https://packages.invalid/effect.demo.json".to_owned(),
                    Ok(metadata),
                ),
                (
                    "https://packages.invalid/effect.demo.zip".to_owned(),
                    Ok(archive),
                ),
            ]),
        };
        let mut model = PackageManagerModel::load_from(
            root.clone(),
            vec![json!({"url":repository,"enabled":true,"priority":1})],
            "en".to_owned(),
            "0.6.2".to_owned(),
        );

        let install = model.execute_operation(
            PackageOperation::Install {
                package_id: "effect.demo".to_owned(),
                source_repository: repository.to_owned(),
                version: String::new(),
            },
            &client,
            |_, _| {},
        );
        assert!(install.errors.is_empty(), "{:?}", install.errors);
        assert_eq!(install.installed, ["effect.demo"]);
        assert!(install.reload_effect_catalog);
        let deployed = root
            .parent()
            .expect("root has parent")
            .join("effects/effect.demo/main.json");
        assert_eq!(
            fs::read_to_string(&deployed).expect("deployed file reads"),
            r#"{"id":"effect.demo"}"#
        );
        let installed = read_json_object(&root.join("installed.json"), MAX_INSTALLED_STATE_BYTES)
            .expect("installed state reads");
        assert_eq!(installed["effect.demo"]["version"], "2.0.0");
        assert!(
            installed["effect.demo"]["installed_at"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z'))
        );

        let remove = model.execute_operation(
            PackageOperation::Remove {
                package_id: "effect.demo".to_owned(),
            },
            &client,
            |_, _| {},
        );
        assert!(remove.errors.is_empty(), "{:?}", remove.errors);
        assert_eq!(remove.removed, ["effect.demo"]);
        assert!(!deployed.exists());
        assert!(
            !read_json_object(&root.join("installed.json"), MAX_INSTALLED_STATE_BYTES,)
                .expect("installed state reads")
                .contains_key("effect.demo")
        );

        fs::remove_dir_all(root.parent().expect("root has parent"))
            .expect("temporary package root removes");
    }

    #[test]
    fn package_deployment_rejects_unsafe_zip_entries_and_restores_on_state_failure() {
        let root = temporary_root();
        let base = root.parent().expect("root has parent");
        let traversal = stored_zip(&[("../escaped.txt", b"escape", 0o100644_u32 << 16)]);
        let destination = base.join("extract");
        assert!(extract_zip_archive(&traversal, &destination).is_err());
        assert!(!base.join("escaped.txt").exists());

        let symlink = stored_zip(&[("link", b"target", 0o120777_u32 << 16)]);
        assert!(extract_zip_archive(&symlink, &destination).is_err());

        let package_dir = base.join("effects/effect.rollback");
        fs::create_dir_all(&package_dir).expect("existing package creates");
        fs::write(package_dir.join("old.txt"), "old").expect("existing package writes");
        let replacement = stored_zip(&[("new.txt", b"new", 0o100644_u32 << 16)]);
        let error =
            deploy_package_archive(&root, "effect.rollback", "effect", &replacement, || {
                Err("simulated state failure".to_owned())
            })
            .expect_err("state failure rolls back");
        assert!(error.contains("previous installation was restored"));
        assert_eq!(
            fs::read_to_string(package_dir.join("old.txt")).expect("backup restores"),
            "old"
        );
        assert!(!package_dir.join("new.txt").exists());

        fs::remove_dir_all(base).expect("temporary package root removes");
    }

    #[test]
    fn plugin_permission_grants_preserve_other_plugins_and_persist_atomically() {
        let root = temporary_root();
        let base = root.parent().expect("root has parent");
        fs::create_dir_all(base).expect("temporary package root creates");
        let settings_path = base.join("settings.json");
        fs::write(
            &settings_path,
            serde_json::to_vec(&json!({
                "pluginPermissions": {
                "mod.demo":["clip.read"],
                "mod.other":["project.read"]
                }
            }))
            .expect("settings serialize"),
        )
        .expect("settings write");
        let (mut settings, _) = SettingsStore::load_from(settings_path.clone(), Map::new());
        let grants = plugin_permission_grants(&settings, "mod.demo");
        assert!(
            grants
                .iter()
                .any(|permission| permission.name == "clip.read" && permission.granted)
        );
        assert!(
            grants
                .iter()
                .any(|permission| permission.name == "clip.modify" && !permission.granted)
        );

        save_plugin_permission_grants(
            &mut settings,
            "mod.demo",
            &["clip.modify".to_owned(), "unknown.permission".to_owned()],
        )
        .expect("permission grants save");
        let persisted = serde_json::from_slice::<Value>(
            &fs::read(settings_path).expect("settings remain readable"),
        )
        .expect("settings remain valid JSON");
        assert_eq!(
            persisted["pluginPermissions"]["mod.demo"],
            json!(["clip.modify"])
        );
        assert_eq!(
            persisted["pluginPermissions"]["mod.other"],
            json!(["project.read"])
        );

        fs::remove_dir_all(base).expect("temporary package root removes");
    }
}
