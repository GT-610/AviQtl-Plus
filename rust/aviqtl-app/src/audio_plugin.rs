use crate::settings::SettingsStore;
use aviqtl_audio::{
    ModernAudioPluginFormat, ModernAudioPluginInfo, inspect_audio_plugin, scan_modern_audio_plugins,
};
use aviqtl_carla::{CarlaLibraryPaths, CarlaPluginInfo, inspect_plugin as inspect_carla_plugin};
use aviqtl_rust_core::api::{
    AudioPluginDocument, AudioPluginInfo, ProjectDocument, audio_plugin_categories,
    normalize_audio_plugin_category, parse_audio_plugin_discovery_output,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    DirectBundles,
    RecursiveFiles,
    RecursiveEntries,
}

#[derive(Debug, Clone)]
struct FormatConfig {
    discovery_type: &'static str,
    format: &'static str,
    extensions: &'static [&'static str],
    target_kind: TargetKind,
}

#[derive(Debug, Clone)]
struct DiscoveryConfig {
    tool_override: Option<PathBuf>,
    resource_root: PathBuf,
    formats: Vec<(FormatConfig, Vec<PathBuf>)>,
    threads: usize,
    timeout: Duration,
}

/// A reusable, GUI-independent plugin scan prepared from the current settings.
#[derive(Debug, Clone)]
pub struct AudioPluginScanner {
    config: DiscoveryConfig,
}

impl AudioPluginScanner {
    pub fn from_settings(settings: &SettingsStore) -> Self {
        let formats = format_configs()
            .into_iter()
            .filter(|format| settings.bool_value(&format!("pluginEnable{}", format.format), true))
            .map(|format| {
                let paths = settings
                    .value(&format!("pluginPaths{}", format.format))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .map(PathBuf::from)
                    .collect();
                (format, paths)
            })
            .collect();
        let default_threads = thread::available_parallelism()
            .map(|threads| threads.get().saturating_sub(1).max(2))
            .unwrap_or(2);
        let configured_threads = settings.i32_value(
            "pluginDiscoveryThreads",
            i32::try_from(default_threads).unwrap_or(i32::MAX),
        );
        let timeout_ms = settings
            .i32_value("pluginDiscoveryTimeoutMs", 5_000)
            .clamp(100, 120_000);
        Self {
            config: DiscoveryConfig {
                tool_override: settings
                    .value("_pluginDiscoveryToolPath")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .map(PathBuf::from),
                resource_root: application_resource_root(),
                formats,
                threads: usize::try_from(configured_threads.clamp(1, 64)).unwrap_or(1),
                timeout: Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(5_000)),
            },
        }
    }

    pub fn scan(&self, stop: &AtomicBool) -> AudioPluginScanOutcome {
        scan_plugins(&self.config, stop)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPluginMenuEntry {
    pub id: String,
    pub name: String,
    pub category: String,
}

#[derive(Debug, Clone)]
struct DiscoveredAudioPlugin {
    info: AudioPluginInfo,
    modern: Option<ModernAudioPluginInfo>,
    carla: Option<CarlaPluginInfo>,
}

impl std::ops::Deref for DiscoveredAudioPlugin {
    type Target = AudioPluginInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

#[derive(Debug, Clone, Default)]
pub struct AudioPluginCatalog {
    plugins: Vec<DiscoveredAudioPlugin>,
    categories: Vec<String>,
    scanned: bool,
}

impl AudioPluginCatalog {
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    pub fn entries(&self, query: &str) -> Vec<AudioPluginMenuEntry> {
        let query = query.trim().to_lowercase();
        let category_order = self
            .categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        let mut entries = self
            .plugins
            .iter()
            .filter(|plugin| plugin.modern.is_some() || plugin.carla.is_some())
            .filter(|plugin| {
                query.is_empty()
                    || [
                        plugin.name.as_str(),
                        plugin.id.as_str(),
                        plugin.category.as_str(),
                        plugin.maker.as_str(),
                        plugin.format.as_str(),
                        plugin.path.as_str(),
                    ]
                    .iter()
                    .any(|value| value.to_lowercase().contains(&query))
            })
            .map(|plugin| AudioPluginMenuEntry {
                id: plugin.id.clone(),
                name: plugin.name.clone(),
                category: normalize_audio_plugin_category(&plugin.category),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            category_order
                .get(left.category.as_str())
                .cmp(&category_order.get(right.category.as_str()))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.id.cmp(&right.id))
        });
        entries
    }

    pub fn addition(&self, id: &str) -> Result<AudioPluginAddition, String> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| format!("audio plugin is unavailable: {id}"))?;
        if let Some(modern) = &plugin.modern {
            build_audio_plugin_document(modern)
        } else if let Some(carla) = &plugin.carla {
            build_carla_audio_plugin_document(carla)
        } else {
            Err(format!("audio plugin cannot be hosted: {id}"))
        }
    }

    pub fn pending_hydration_count(&self, document: &ProjectDocument) -> usize {
        if !self.scanned {
            return 0;
        }
        document
            .clips
            .iter()
            .flat_map(|clip| &clip.audio_plugins)
            .filter(|plugin| audio_plugin_needs_hydration(plugin))
            .filter(|plugin| {
                self.plugins.iter().any(|candidate| {
                    candidate.id == plugin.id
                        && (candidate.modern.is_some() || candidate.carla.is_some())
                })
            })
            .count()
    }

    pub fn hydrate_document(&self, document: &mut ProjectDocument) -> AudioPluginHydration {
        if !self.scanned {
            return AudioPluginHydration::default();
        }
        let ids = document
            .clips
            .iter()
            .flat_map(|clip| &clip.audio_plugins)
            .filter(|plugin| audio_plugin_needs_hydration(plugin))
            .map(|plugin| plugin.id.clone())
            .collect::<BTreeSet<_>>();
        let additions = ids
            .into_iter()
            .map(|id| {
                let result = self.addition(&id);
                (id, result)
            })
            .collect::<BTreeMap<_, _>>();
        let mut result = AudioPluginHydration::default();
        for plugin in document
            .clips
            .iter_mut()
            .flat_map(|clip| &mut clip.audio_plugins)
            .filter(|plugin| audio_plugin_needs_hydration(plugin))
        {
            let Some(addition) = additions.get(&plugin.id) else {
                continue;
            };
            match addition {
                Ok(addition) => {
                    if merge_audio_plugin_document(plugin, &addition.plugin) {
                        result.hydrated += 1;
                    }
                }
                Err(error) => {
                    if !result.errors.contains(error) {
                        result.errors.push(error.clone());
                    }
                }
            }
        }
        result
    }
}

#[derive(Debug)]
pub struct AudioPluginAddition {
    pub plugin: AudioPluginDocument,
    pub display_name: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AudioPluginHydration {
    pub hydrated: usize,
    pub deferred: usize,
    pub errors: Vec<String>,
}

#[derive(Debug)]
pub struct AudioPluginScanOutcome {
    pub catalog: AudioPluginCatalog,
    pub diagnostics: Vec<String>,
    pub elapsed: Duration,
    pub stopped: bool,
}

impl AudioPluginScanOutcome {
    pub fn status(&self) -> String {
        let detail = if self.stopped {
            "stopped".to_owned()
        } else if self.diagnostics.is_empty() {
            format!("{} found", self.catalog.plugins.len())
        } else {
            format!(
                "{} found · {} diagnostics",
                self.catalog.plugins.len(),
                self.diagnostics.len()
            )
        };
        format!("Audio plugins · {detail} · {:.2?}", self.elapsed)
    }
}

fn audio_plugin_needs_hydration(plugin: &AudioPluginDocument) -> bool {
    plugin.extra.get("path").and_then(Value::as_str).is_none()
        || plugin
            .extra
            .get("parameterInfo")
            .and_then(Value::as_array)
            .is_none()
}

fn merge_audio_plugin_document(
    plugin: &mut AudioPluginDocument,
    discovered: &AudioPluginDocument,
) -> bool {
    let mut changed = false;
    for (key, value) in &discovered.extra {
        if !plugin.extra.contains_key(key) {
            plugin.extra.insert(key.clone(), value.clone());
            changed = true;
        }
    }
    for (key, value) in &discovered.params {
        if !plugin.params.contains_key(key) {
            plugin.params.insert(key.clone(), value.clone());
            changed = true;
        }
    }
    changed
}

fn modern_plugin_entry(plugin: ModernAudioPluginInfo) -> DiscoveredAudioPlugin {
    let info = AudioPluginInfo {
        id: plugin.id.clone(),
        name: plugin.name.clone(),
        format: plugin.format.display_name().to_owned(),
        category: plugin.category.clone(),
        path: plugin.path.to_string_lossy().into_owned(),
        label: plugin.native_id.clone(),
        maker: plugin.vendor.clone(),
        unique_id: 0,
        index: 0,
        audio_ins: 2,
        audio_outs: 2,
    };
    DiscoveredAudioPlugin {
        info,
        modern: Some(plugin),
        carla: None,
    }
}

fn build_audio_plugin_document(
    info: &ModernAudioPluginInfo,
) -> Result<AudioPluginAddition, String> {
    let description = inspect_audio_plugin(info)
        .map_err(|error| format!("failed to load {}: {error}", info.name))?;
    let mut params = Map::new();
    let parameter_info = description
        .parameters
        .iter()
        .map(|parameter| {
            if !parameter.read_only {
                params.insert(
                    parameter.index.to_string(),
                    Value::from(parameter.default_value),
                );
            }
            json!({
                "index": parameter.index,
                "name": parameter.name,
                "unit": parameter.unit,
                "minimum": parameter.minimum,
                "maximum": parameter.maximum,
                "default": parameter.default_value,
                "stepCount": parameter.step_count,
                "readOnly": parameter.read_only,
                "hidden": parameter.hidden
            })
        })
        .collect::<Vec<_>>();
    let extra = BTreeMap::from([
        ("hostBackend".to_owned(), json!("truce-rack")),
        (
            "format".to_owned(),
            json!(description.info.format.display_name()),
        ),
        (
            "path".to_owned(),
            json!(description.info.path.to_string_lossy()),
        ),
        ("nativeId".to_owned(), json!(description.info.native_id)),
        ("name".to_owned(), json!(description.info.name)),
        ("vendor".to_owned(), json!(description.info.vendor)),
        ("category".to_owned(), json!(description.info.category)),
        ("version".to_owned(), json!(description.info.version)),
        ("hasEditor".to_owned(), json!(description.info.has_editor)),
        ("parameterInfo".to_owned(), Value::Array(parameter_info)),
    ]);
    Ok(AudioPluginAddition {
        plugin: AudioPluginDocument {
            id: description.info.id,
            enabled: true,
            params,
            keyframes: None,
            extra,
        },
        display_name: info.name.clone(),
    })
}

fn carla_plugin_entry(
    plugin: AudioPluginInfo,
    libraries: &CarlaLibraryPaths,
) -> DiscoveredAudioPlugin {
    let carla = CarlaPluginInfo::supports_format(&plugin.format).then(|| CarlaPluginInfo {
        id: plugin.id.clone(),
        name: plugin.name.clone(),
        format: plugin.format.clone(),
        category: plugin.category.clone(),
        path: PathBuf::from(&plugin.path),
        label: plugin.label.clone(),
        maker: plugin.maker.clone(),
        unique_id: plugin.unique_id,
        index: plugin.index,
        libraries: libraries.clone(),
    });
    DiscoveredAudioPlugin {
        info: plugin,
        modern: None,
        carla,
    }
}

fn build_carla_audio_plugin_document(
    info: &CarlaPluginInfo,
) -> Result<AudioPluginAddition, String> {
    let description = inspect_carla_plugin(info, 48_000, 1_024)
        .map_err(|error| format!("failed to load {} through Carla: {error}", info.name))?;
    let mut params = Map::new();
    let parameter_info = description
        .parameters
        .iter()
        .map(|parameter| {
            params.insert(
                parameter.index.to_string(),
                Value::from(parameter.current_value),
            );
            json!({
                "index": parameter.index,
                "name": parameter.name,
                "unit": parameter.unit,
                "minimum": parameter.minimum,
                "maximum": parameter.maximum,
                "default": parameter.default_value,
                "stepCount": 0,
                "readOnly": false,
                "hidden": false
            })
        })
        .collect::<Vec<_>>();
    let extra = BTreeMap::from([
        ("hostBackend".to_owned(), json!("carla")),
        ("format".to_owned(), json!(description.info.format)),
        (
            "path".to_owned(),
            json!(description.info.path.to_string_lossy()),
        ),
        ("label".to_owned(), json!(description.info.label)),
        ("uniqueId".to_owned(), json!(description.info.unique_id)),
        ("index".to_owned(), json!(description.info.index)),
        ("name".to_owned(), json!(description.info.name)),
        ("vendor".to_owned(), json!(description.info.maker)),
        ("category".to_owned(), json!(description.info.category)),
        (
            "carlaNativeLibrary".to_owned(),
            json!(description.info.libraries.native_plugin.to_string_lossy()),
        ),
        (
            "carlaHostLibrary".to_owned(),
            json!(description.info.libraries.host_plugin.to_string_lossy()),
        ),
        (
            "carlaResourceDir".to_owned(),
            json!(description.info.libraries.resource_dir.to_string_lossy()),
        ),
        ("parameterInfo".to_owned(), Value::Array(parameter_info)),
    ]);
    Ok(AudioPluginAddition {
        plugin: AudioPluginDocument {
            id: description.info.id,
            enabled: true,
            params,
            keyframes: None,
            extra,
        },
        display_name: info.name.clone(),
    })
}

fn scan_plugins(config: &DiscoveryConfig, stop: &AtomicBool) -> AudioPluginScanOutcome {
    let started = Instant::now();
    let mut diagnostics = Vec::new();
    let tool = find_discovery_tool(config);
    let carla_libraries = CarlaLibraryPaths::discover(tool.as_deref());
    let mut plugins = Vec::new();
    for (format, configured_paths) in &config.formats {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(modern_format) = ModernAudioPluginFormat::from_name(format.format) {
            let targets = collect_targets(configured_paths, &config.resource_root, format, stop);
            let directories = targets
                .iter()
                .filter_map(|target| target.parent().map(Path::to_path_buf))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let (found, mut format_diagnostics) =
                scan_modern_audio_plugins(modern_format, &directories);
            plugins.extend(found.into_iter().map(modern_plugin_entry));
            diagnostics.append(&mut format_diagnostics);
            continue;
        }
        let Some(tool) = tool.as_deref() else {
            diagnostics.push(format!(
                "{} discovery requires carla-discovery-native",
                format.format
            ));
            continue;
        };
        if !discovery_type_is_supported(tool, format.discovery_type, stop) {
            diagnostics.push(format!(
                "{} is not supported by {}",
                format.format,
                tool.display()
            ));
            continue;
        }
        let targets = collect_targets(configured_paths, &config.resource_root, format, stop);
        let (found, mut format_diagnostics) =
            discover_targets(tool, format, targets, config.threads, config.timeout, stop);
        if let Some(libraries) = &carla_libraries {
            plugins.extend(
                found
                    .into_iter()
                    .map(|info| carla_plugin_entry(info, libraries)),
            );
        } else {
            diagnostics.push(format!(
                "{} plugins were found but the Carla host libraries are unavailable",
                format.format
            ));
            plugins.extend(found.into_iter().map(|info| DiscoveredAudioPlugin {
                info,
                modern: None,
                carla: None,
            }));
        }
        diagnostics.append(&mut format_diagnostics);
    }
    let mut ids = BTreeSet::new();
    plugins.retain(|plugin| ids.insert(plugin.id.clone()));
    let categories = audio_plugin_categories(
        &plugins
            .iter()
            .map(|plugin| plugin.info.clone())
            .collect::<Vec<_>>(),
    );
    AudioPluginScanOutcome {
        catalog: AudioPluginCatalog {
            plugins,
            categories,
            scanned: true,
        },
        diagnostics,
        elapsed: started.elapsed(),
        stopped: stop.load(Ordering::Relaxed),
    }
}

fn discover_targets(
    tool: &Path,
    format: &FormatConfig,
    targets: Vec<PathBuf>,
    thread_count: usize,
    timeout: Duration,
    stop: &AtomicBool,
) -> (Vec<AudioPluginInfo>, Vec<String>) {
    let queue = Arc::new(Mutex::new(VecDeque::from(targets)));
    let results = Arc::new(Mutex::new(Vec::new()));
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let workers = thread_count
        .max(1)
        .min(queue.lock().map(|queue| queue.len().max(1)).unwrap_or(1));
    thread::scope(|scope| {
        for _ in 0..workers {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let diagnostics = Arc::clone(&diagnostics);
            scope.spawn(move || {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let target = queue.lock().ok().and_then(|mut queue| queue.pop_front());
                    let Some(target) = target else {
                        break;
                    };
                    match run_discovery(tool, format, &target, timeout, stop) {
                        Ok(mut plugins) => {
                            if let Ok(mut results) = results.lock() {
                                results.append(&mut plugins);
                            }
                        }
                        Err(error) => {
                            if let Ok(mut diagnostics) = diagnostics.lock() {
                                diagnostics.push(error);
                            }
                        }
                    }
                }
            });
        }
    });
    let mut results = results
        .lock()
        .map(|results| results.clone())
        .unwrap_or_default();
    results.sort_by(|left, right| {
        left.format
            .cmp(&right.format)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    let diagnostics = diagnostics
        .lock()
        .map(|diagnostics| diagnostics.clone())
        .unwrap_or_default();
    (results, diagnostics)
}

fn run_discovery(
    tool: &Path,
    format: &FormatConfig,
    target: &Path,
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<Vec<AudioPluginInfo>, String> {
    let output = run_process(
        tool,
        &[format.discovery_type, &target.to_string_lossy()],
        timeout,
        stop,
    )?;
    let transcript = if output.stdout.contains("carla-discovery::") {
        &output.stdout
    } else if output.stderr.contains("carla-discovery::") {
        &output.stderr
    } else {
        return Ok(Vec::new());
    };
    let fallback_name = target
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    Ok(parse_audio_plugin_discovery_output(
        transcript,
        format.format,
        &target.to_string_lossy(),
        fallback_name,
    ))
}

struct ProcessOutput {
    stdout: String,
    stderr: String,
}

fn run_process(
    tool: &Path,
    arguments: &[&str],
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<ProcessOutput, String> {
    let mut child = Command::new(tool)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", tool.display()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));
    let started = Instant::now();
    let mut timed_out = false;
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("{}: {error}", tool.display()))?
            .is_some()
        {
            break;
        }
        if stop.load(Ordering::Relaxed) || started.elapsed() >= timeout {
            timed_out = started.elapsed() >= timeout;
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if timed_out {
        return Err(format!(
            "{} timed out after {} ms",
            arguments.last().copied().unwrap_or_default(),
            timeout.as_millis()
        ));
    }
    Ok(ProcessOutput { stdout, stderr })
}

fn read_pipe<R: Read>(pipe: Option<R>) -> String {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn discovery_type_is_supported(tool: &Path, discovery_type: &str, stop: &AtomicBool) -> bool {
    run_process(tool, &[discovery_type, "__probe__"], PROBE_TIMEOUT, stop)
        .is_ok_and(|output| !output.stderr.contains("invalid string type"))
}

fn collect_targets(
    configured_paths: &[PathBuf],
    resource_root: &Path,
    format: &FormatConfig,
    stop: &AtomicBool,
) -> Vec<PathBuf> {
    let mut visited_roots = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for configured in configured_paths {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let path = if configured.is_absolute() {
            configured.clone()
        } else {
            resource_root.join(configured)
        };
        let Ok(root) = path.canonicalize() else {
            continue;
        };
        if !root.is_dir() || !visited_roots.insert(root.clone()) {
            continue;
        }
        match format.target_kind {
            TargetKind::DirectBundles => {
                collect_direct_entries(&root, format.extensions, true, &mut targets)
            }
            TargetKind::RecursiveFiles => {
                collect_recursive_entries(&root, format.extensions, false, stop, &mut targets)
            }
            TargetKind::RecursiveEntries => {
                collect_recursive_entries(&root, format.extensions, true, stop, &mut targets)
            }
        }
    }
    targets.into_iter().collect()
}

fn collect_direct_entries(
    root: &Path,
    extensions: &[&str],
    allow_directories: bool,
    targets: &mut BTreeSet<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if extension_matches(&path, extensions)
            && (path.is_file() || (allow_directories && path.is_dir()))
        {
            targets.insert(path);
        }
    }
}

fn collect_recursive_entries(
    root: &Path,
    extensions: &[&str],
    allow_directories: bool,
    stop: &AtomicBool,
    targets: &mut BTreeSet<PathBuf>,
) {
    let mut directories = vec![root.to_path_buf()];
    let mut visited = BTreeSet::new();
    while let Some(directory) = directories.pop() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let canonical = directory.canonicalize().unwrap_or(directory.clone());
        if !visited.insert(canonical) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if allow_directories && extension_matches(&path, extensions) {
                    targets.insert(path);
                } else {
                    directories.push(path);
                }
            } else if file_type.is_file() && extension_matches(&path, extensions) {
                targets.insert(path);
            }
        }
    }
}

fn extension_matches(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn find_discovery_tool(config: &DiscoveryConfig) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = &config.tool_override {
        candidates.push(path.clone());
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        candidates.push(directory.join(discovery_tool_name()));
    }
    for path in [
        "/usr/lib/carla/carla-discovery-native",
        "/usr/local/lib/carla/carla-discovery-native",
        "/usr/lib64/carla/carla-discovery-native",
        "/usr/bin/carla-discovery-native",
        "/usr/local/bin/carla-discovery-native",
        "/opt/homebrew/bin/carla-discovery-native",
        "/opt/homebrew/opt/carla/lib/carla/carla-discovery-native",
    ] {
        candidates.push(PathBuf::from(path));
    }
    if let Some(path) = executable_in_path(discovery_tool_name()) {
        candidates.push(path);
    }
    candidates.into_iter().find(|path| is_executable(path))
}

fn executable_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|path| path.join(name))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn discovery_tool_name() -> &'static str {
    if cfg!(windows) {
        "carla-discovery-native.exe"
    } else {
        "carla-discovery-native"
    }
}

fn application_resource_root() -> PathBuf {
    let executable_directory = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    if let Ok(resources) = executable_directory.join("../Resources").canonicalize() {
        return resources;
    }
    if executable_directory
        .file_name()
        .and_then(|name| name.to_str())
        == Some("bin")
        && let Some(parent) = executable_directory.parent()
    {
        return parent.to_path_buf();
    }
    executable_directory
}

fn format_configs() -> Vec<FormatConfig> {
    let dynamic_library = if cfg!(target_os = "windows") {
        &["dll"][..]
    } else if cfg!(target_os = "macos") {
        &["dylib", "so"][..]
    } else {
        &["so"][..]
    };
    let vst2 = if cfg!(target_os = "windows") {
        FormatConfig {
            discovery_type: "vst2",
            format: "VST2",
            extensions: &["dll"],
            target_kind: TargetKind::RecursiveFiles,
        }
    } else if cfg!(target_os = "macos") {
        FormatConfig {
            discovery_type: "vst2",
            format: "VST2",
            extensions: &["vst"],
            target_kind: TargetKind::RecursiveEntries,
        }
    } else {
        FormatConfig {
            discovery_type: "vst2",
            format: "VST2",
            extensions: &["so"],
            target_kind: TargetKind::RecursiveFiles,
        }
    };
    vec![
        FormatConfig {
            discovery_type: "ladspa",
            format: "LADSPA",
            extensions: dynamic_library,
            target_kind: TargetKind::RecursiveFiles,
        },
        FormatConfig {
            discovery_type: "dssi",
            format: "DSSI",
            extensions: dynamic_library,
            target_kind: TargetKind::RecursiveFiles,
        },
        FormatConfig {
            discovery_type: "lv2",
            format: "LV2",
            extensions: &["lv2"],
            target_kind: TargetKind::DirectBundles,
        },
        vst2,
        FormatConfig {
            discovery_type: "vst3",
            format: "VST3",
            extensions: &["vst3"],
            target_kind: TargetKind::RecursiveEntries,
        },
        FormatConfig {
            discovery_type: "clap",
            format: "CLAP",
            extensions: &["clap"],
            target_kind: TargetKind::RecursiveEntries,
        },
        FormatConfig {
            discovery_type: "sf2",
            format: "SF2",
            extensions: &["sf2"],
            target_kind: TargetKind::RecursiveFiles,
        },
        FormatConfig {
            discovery_type: "sfz",
            format: "SFZ",
            extensions: &["sfz"],
            target_kind: TargetKind::RecursiveFiles,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn temporary_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aviqtl-audio-discovery-{}-{nonce}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temporary directory");
        path
    }

    #[test]
    fn target_collection_handles_bundles_recursive_files_and_duplicate_roots() {
        let root = temporary_directory();
        let nested = root.join("nested");
        fs::create_dir_all(root.join("Example.vst3")).expect("bundle directory");
        fs::create_dir_all(&nested).expect("nested directory");
        fs::write(nested.join("effect.clap"), b"fixture").expect("plugin fixture");
        fs::write(nested.join("ignore.txt"), b"fixture").expect("ignored fixture");
        let stop = AtomicBool::new(false);
        let canonical_root = root.canonicalize().expect("canonical root");

        let bundles = collect_targets(
            &[root.clone(), root.clone()],
            Path::new("/unused"),
            &FormatConfig {
                discovery_type: "vst3",
                format: "VST3",
                extensions: &["vst3"],
                target_kind: TargetKind::RecursiveEntries,
            },
            &stop,
        );
        assert_eq!(bundles, vec![canonical_root.join("Example.vst3")]);

        let files = collect_targets(
            std::slice::from_ref(&root),
            Path::new("/unused"),
            &FormatConfig {
                discovery_type: "clap",
                format: "CLAP",
                extensions: &["clap"],
                target_kind: TargetKind::RecursiveFiles,
            },
            &stop,
        );
        assert_eq!(files, vec![canonical_root.join("nested/effect.clap")]);

        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    #[test]
    fn hydration_preserves_saved_values_and_adds_missing_host_metadata() {
        let mut saved = AudioPluginDocument {
            id: "fixture".to_owned(),
            enabled: false,
            params: json!({"0": 0.75}).as_object().cloned().expect("params"),
            keyframes: None,
            extra: BTreeMap::from([("savedExtension".to_owned(), json!(7))]),
        };
        let discovered = AudioPluginDocument {
            id: "fixture".to_owned(),
            enabled: true,
            params: json!({"0": 0.5, "1": 1.0})
                .as_object()
                .cloned()
                .expect("params"),
            keyframes: None,
            extra: BTreeMap::from([
                ("path".to_owned(), json!("/plugins/fixture.clap")),
                ("parameterInfo".to_owned(), json!([])),
            ]),
        };

        assert!(merge_audio_plugin_document(&mut saved, &discovered));
        assert_eq!(saved.params["0"], json!(0.75));
        assert_eq!(saved.params["1"], json!(1.0));
        assert_eq!(saved.extra["savedExtension"], json!(7));
        assert_eq!(saved.extra["path"], json!("/plugins/fixture.clap"));
        assert!(!saved.enabled);
    }

    #[test]
    fn catalog_entries_preserve_category_order_and_search_all_qt_fields() {
        let plugin = |id: &str, name: &str, category: &str, vendor: &str| {
            modern_plugin_entry(ModernAudioPluginInfo {
                id: id.to_owned(),
                name: name.to_owned(),
                format: ModernAudioPluginFormat::Clap,
                category: category.to_owned(),
                path: PathBuf::from(format!("/plugins/{id}.clap")),
                native_id: id.to_owned(),
                vendor: vendor.to_owned(),
                version: 1,
                has_editor: false,
            })
        };
        let catalog = AudioPluginCatalog {
            plugins: vec![
                plugin("meter", "Meter", "Utility", "AviQtl"),
                plugin("gain-b", "Gain B", "Effect", "Vendor B"),
                plugin("gain-a", "Gain A", "Effect", "Vendor A"),
            ],
            categories: vec!["Effect".to_owned(), "Utility".to_owned()],
            scanned: true,
        };

        assert_eq!(
            catalog
                .entries("")
                .into_iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            ["gain-a", "gain-b", "meter"]
        );
        assert_eq!(catalog.entries("aviqtl")[0].id, "meter");
        assert_eq!(catalog.entries("CLAP").len(), 3);
        assert_eq!(catalog.entries("gain-b.clap")[0].id, "gain-b");

        let uncategorized = AudioPluginCatalog {
            plugins: vec![plugin("unknown", "Unknown", "misc", "Vendor")],
            categories: vec!["Other".to_owned()],
            scanned: true,
        };
        assert_eq!(uncategorized.entries("")[0].category, "Other");
    }

    #[test]
    fn pending_hydration_defers_without_invalidating_existing_undo_history() {
        use crate::{ProjectSession, WorkspaceModel};
        use aviqtl_rust_core::api::TimelineCommand;

        let catalog = AudioPluginCatalog {
            plugins: vec![modern_plugin_entry(ModernAudioPluginInfo {
                id: "CLAP:fixture:0".to_owned(),
                name: "Fixture Gain".to_owned(),
                format: ModernAudioPluginFormat::Clap,
                category: "Effect".to_owned(),
                path: PathBuf::from("/plugins/fixture.clap"),
                native_id: "fixture".to_owned(),
                vendor: "AviQtl".to_owned(),
                version: 1,
                has_editor: false,
            })],
            categories: vec!["Effect".to_owned()],
            scanned: true,
        };
        let project = ProjectSession::from_json(
            br#"{
                "version":3,
                "settings":{"width":1920,"height":1080,"fps":60,"sampleRate":48000},
                "scenes":[{"id":1,"name":"Root","duration":300}],
                "clips":[{
                    "id":9,"sceneId":1,"type":"audio","start":20,"duration":100,"layer":0,
                    "effects":[{"id":"audio","name":"Audio","params":{"volume":1.0}}],
                    "audioPlugins":[{"id":"CLAP:fixture:0","enabled":true,"params":{"0":0.25}}]
                }]
            }"#,
        )
        .expect("audio project");
        let mut workspace = WorkspaceModel::new(project);
        assert!(workspace.execute(TimelineCommand::SetAudioPluginEnabled {
            clip_id: 9,
            plugin_index: 0,
            enabled: false,
        }));

        let hydration = workspace.hydrate_audio_plugins(&catalog);
        assert_eq!(hydration.hydrated, 0);
        assert_eq!(hydration.deferred, 1);
        assert!(hydration.errors.is_empty());
        assert!(
            !workspace.document().clips[0].audio_plugins[0]
                .extra
                .contains_key("path")
        );
        assert!(workspace.undo());
        assert!(workspace.document().clips[0].audio_plugins[0].enabled);
    }
}
