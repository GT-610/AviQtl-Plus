use crate::ApplicationModel;
use crate::effect_catalog::EffectCatalog;
use crate::project_io::ProjectDefaults;
use crate::settings::{SettingsStore, package_paths};
use aviqtl_rust_core::api::{
    PluginPermissionState, ScriptClipSnapshot, ScriptExecution, ScriptHook, ScriptHostCommand,
    ScriptHostSnapshot, ScriptPluginIdentity, ScriptPluginValidationStatus, ScriptRuntime,
    inspect_script_metadata, parse_script_plugin_manifest, validate_script_plugin_manifest,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

const MAX_PLUGIN_SCRIPT_BYTES: u64 = 8 * 1024 * 1024;
const HOT_RELOAD_POLL_MS: u64 = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModTickOutcome {
    pub applied_commands: usize,
    pub diagnostics: Vec<String>,
    pub model_changed: bool,
    pub settings_changed: bool,
    pub reloaded: bool,
}

struct LoadedPlugin {
    id: String,
    path: PathBuf,
    parameters: BTreeMap<String, Value>,
    runtime: ScriptRuntime,
}

pub struct ModHost {
    plugins: Vec<LoadedPlugin>,
    permissions: PluginPermissionState,
    pending: Vec<ScriptExecution>,
    diagnostics: Vec<String>,
    watched_files: Vec<(PathBuf, SystemTime)>,
    last_update: Instant,
    last_hot_check: Instant,
    last_project_key: Option<(Option<u64>, Option<PathBuf>, u64)>,
    last_clips_signature: u64,
    last_dirty: bool,
    dispatching: bool,
}

impl ModHost {
    pub fn load(settings: &SettingsStore) -> Self {
        Self::load_with_snapshot(settings, ScriptHostSnapshot::default())
    }

    fn load_with_snapshot(settings: &SettingsStore, snapshot: ScriptHostSnapshot) -> Self {
        let permissions = permissions_from(settings);
        let mut host = Self {
            plugins: Vec::new(),
            permissions,
            pending: Vec::new(),
            diagnostics: Vec::new(),
            watched_files: Vec::new(),
            last_update: Instant::now(),
            last_hot_check: Instant::now(),
            last_project_key: None,
            last_clips_signature: 0,
            last_dirty: false,
            dispatching: false,
        };
        host.discover_and_load(settings, &snapshot);
        // Dispatch Load hooks with the same snapshot Qt uses at startup
        // (no project is open yet, so this is usually the default snapshot).
        host.dispatch_and_queue(ScriptHook::Load, settings, &snapshot);
        host
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn take_diagnostics(&mut self) -> Vec<String> {
        std::mem::take(&mut self.diagnostics)
    }

    fn discover_and_load(&mut self, settings: &SettingsStore, snapshot: &ScriptHostSnapshot) {
        let paths = package_paths();
        let mut candidates = Vec::new();
        for root in paths.plugin_roots {
            candidates.extend(discover_in_root(&root));
        }
        candidates.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.path.cmp(&right.path))
        });
        candidates.dedup_by(|right, left| right.id == left.id && right.path == left.path);

        let mut loaded_identities: Vec<ScriptPluginIdentity> = Vec::new();
        let mut watched = BTreeSet::new();
        for candidate in candidates {
            let path_identity = canonical_identity(&candidate.path);
            let manifest = match validate_candidate(
                candidate.manifest,
                candidate.single_file,
                &candidate.expected_id,
                &path_identity,
                &loaded_identities,
            ) {
                Some(manifest) => manifest,
                None => {
                    self.diagnostics.push(format!(
                        "Skipping MOD with an invalid manifest: {}",
                        candidate.path.display()
                    ));
                    continue;
                }
            };
            let source = match read_bounded(&candidate.path) {
                Some(source) => source,
                None => {
                    self.diagnostics.push(format!(
                        "Skipping unreadable MOD script: {}",
                        candidate.path.display()
                    ));
                    continue;
                }
            };
            let parameters = resolve_parameters(
                settings,
                &manifest.id,
                candidate.single_file,
                &candidate.path,
                &source,
            );
            let plugin_snapshot = snapshot_for_plugin(snapshot, settings, &manifest.id);
            match ScriptRuntime::load(
                manifest.id.clone(),
                &source,
                &candidate.path.display().to_string(),
                &parameters,
                &self.permissions,
                plugin_snapshot,
            ) {
                Ok((runtime, initial)) => {
                    if !initial.diagnostics.is_empty() {
                        for diagnostic in &initial.diagnostics {
                            self.diagnostics
                                .push(format!("{} top-level: {diagnostic}", manifest.id));
                        }
                    }
                    if !initial.commands.is_empty() {
                        self.pending.push(initial);
                    }
                    loaded_identities.push(ScriptPluginIdentity {
                        id: manifest.id.clone(),
                        path: path_identity,
                    });
                    if let Some(mtime) = file_mtime(&candidate.path) {
                        watched.insert((candidate.path.clone(), mtime));
                    }
                    // Manifest files also participate in hot reload.
                    if let Some(manifest_path) = candidate.manifest_path
                        && let Some(mtime) = file_mtime(&manifest_path)
                    {
                        watched.insert((manifest_path, mtime));
                    }
                    self.plugins.push(LoadedPlugin {
                        id: manifest.id.clone(),
                        path: candidate.path,
                        parameters,
                        runtime,
                    });
                    let _ = manifest;
                }
                Err(error) => {
                    self.diagnostics.push(format!(
                        "Failed to load MOD {}: {error}",
                        candidate.path.display()
                    ));
                }
            }
        }
        let mut watched: Vec<(PathBuf, SystemTime)> = watched.into_iter().collect();
        watched.sort();
        self.watched_files = watched;
    }

    fn dispatch_and_queue(
        &mut self,
        hook: ScriptHook,
        settings: &SettingsStore,
        base_snapshot: &ScriptHostSnapshot,
    ) {
        if self.dispatching {
            return;
        }
        self.dispatching = true;
        for plugin in &mut self.plugins {
            let snapshot = snapshot_for_plugin(base_snapshot, settings, &plugin.id);
            // Refresh injected globals if the stored settings changed externally.
            let _ = plugin.runtime.set_parameters(&plugin.parameters);
            let execution = plugin
                .runtime
                .dispatch(hook_clone(&hook), &self.permissions, snapshot);
            if !execution.diagnostics.is_empty() || !execution.commands.is_empty() {
                self.pending.push(execution);
            }
        }
        self.dispatching = false;
    }

    fn dispatch_hook_for_all(
        &mut self,
        hook: ScriptHook,
        model: &ApplicationModel,
        settings: &SettingsStore,
    ) {
        let base = base_snapshot(model);
        self.dispatch_and_queue(hook, settings, &base);
    }

    pub fn apply_pending(
        &mut self,
        model: &mut ApplicationModel,
        settings: &mut SettingsStore,
        catalog: &EffectCatalog,
        defaults: &ProjectDefaults,
        max_layers: i32,
        default_duration: i32,
    ) -> ModTickOutcome {
        let mut outcome = ModTickOutcome::default();
        let pending = std::mem::take(&mut self.pending);
        for execution in pending {
            for diagnostic in execution.diagnostics {
                outcome
                    .diagnostics
                    .push(format!("{}: {diagnostic}", execution.plugin_id));
            }
            for command in execution.commands {
                let plugin_id = execution.plugin_id.clone();
                match apply_command(
                    model,
                    settings,
                    catalog,
                    defaults,
                    max_layers,
                    default_duration,
                    &plugin_id,
                    command,
                ) {
                    Ok(changed) => {
                        outcome.applied_commands += 1;
                        outcome.model_changed |= changed.model_changed;
                        outcome.settings_changed |= changed.settings_changed;
                    }
                    Err(error) => {
                        outcome.diagnostics.push(format!("{plugin_id}: {error}"));
                    }
                }
            }
        }
        let diagnostics = self.take_diagnostics();
        outcome.diagnostics.extend(diagnostics);
        outcome
    }

    pub fn tick(
        &mut self,
        model: &mut ApplicationModel,
        settings: &mut SettingsStore,
        catalog: &EffectCatalog,
        defaults: &ProjectDefaults,
        max_layers: i32,
        default_duration: i32,
    ) -> ModTickOutcome {
        let mut outcome = ModTickOutcome::default();
        // Hot reload polling mirrors the Qt 500ms debounce at a coarse level:
        // any watched file change triggers unload + load + Load hooks.
        if settings.bool_value("luaHotReload", false)
            && self.last_hot_check.elapsed().as_millis() as u64 >= HOT_RELOAD_POLL_MS
        {
            self.last_hot_check = Instant::now();
            if self.watched_files_changed() {
                let base = base_snapshot(model);
                self.dispatch_and_queue(ScriptHook::Unload, settings, &base);
                let unload = self.apply_pending(
                    model,
                    settings,
                    catalog,
                    defaults,
                    max_layers,
                    default_duration,
                );
                outcome.applied_commands += unload.applied_commands;
                outcome.diagnostics.extend(unload.diagnostics);
                outcome.model_changed |= unload.model_changed;
                outcome.settings_changed |= unload.settings_changed;
                self.permissions = permissions_from(settings);
                self.plugins.clear();
                self.discover_and_load(settings, &base);
                self.dispatch_and_queue(ScriptHook::Load, settings, &base);
                outcome.reloaded = true;
            }
        }

        let interval_ms = settings.i32_value("luaHookIntervalMs", 16).clamp(1, 1_000) as u64;
        let current_key = project_key(model);
        let current_signature = clips_signature(model);
        let dirty = model
            .current_workspace()
            .is_some_and(|workspace| workspace.project().dirty);

        // Project open/save detection: Qt emits these from explicit project
        // actions. Here we infer them from instance/path/revision changes so
        // existing UI callbacks do not need to be modified.
        if self.last_project_key.is_none() {
            self.last_project_key = Some(current_key.clone());
            self.last_clips_signature = current_signature;
            self.last_dirty = dirty;
            if current_key.0.is_some()
                && let Some(path) = current_key.1.clone()
            {
                self.dispatch_hook_for_all(
                    ScriptHook::ProjectOpen(path.display().to_string()),
                    model,
                    settings,
                );
            }
        } else {
            let previous = self.last_project_key.clone().unwrap_or_default();
            if previous.0 != current_key.0 || previous.1 != current_key.1 {
                if let Some(path) = current_key.1.clone() {
                    self.dispatch_hook_for_all(
                        ScriptHook::ProjectOpen(path.display().to_string()),
                        model,
                        settings,
                    );
                } else if current_key.0.is_some() {
                    // Pathless project opened (new blank project).
                    self.dispatch_hook_for_all(
                        ScriptHook::ProjectOpen(String::new()),
                        model,
                        settings,
                    );
                }
                self.last_project_key = Some(current_key.clone());
                self.last_clips_signature = current_signature;
            } else {
                // Save detection: dirty -> clean with a known path.
                if self.last_dirty
                    && !dirty
                    && let Some(path) = current_key.1.clone()
                {
                    self.dispatch_hook_for_all(
                        ScriptHook::ProjectSave(path.display().to_string()),
                        model,
                        settings,
                    );
                }
                // Clip change detection via content hash.
                if self.last_clips_signature != current_signature {
                    self.dispatch_hook_for_all(ScriptHook::ClipChange, model, settings);
                    self.last_clips_signature = current_signature;
                }
                self.last_project_key = Some(current_key);
            }
            self.last_dirty = dirty;
        }

        if self.last_update.elapsed().as_millis() as u64 >= interval_ms {
            self.last_update = Instant::now();
            let base = base_snapshot(model);
            self.dispatch_and_queue(ScriptHook::Update, settings, &base);
        }

        let applied = self.apply_pending(
            model,
            settings,
            catalog,
            defaults,
            max_layers,
            default_duration,
        );
        outcome.applied_commands += applied.applied_commands;
        outcome.diagnostics.extend(applied.diagnostics);
        outcome.model_changed |= applied.model_changed;
        outcome.settings_changed |= applied.settings_changed;
        // Keep the clip signature fresh when script commands mutated the model
        // so we do not immediately re-emit ClipChange for our own edits.
        if outcome.model_changed {
            self.last_clips_signature = clips_signature(model);
            // Refresh the project key in case scripts saved/loaded projects.
            self.last_project_key = Some(project_key(model));
            self.last_dirty = model
                .current_workspace()
                .is_some_and(|workspace| workspace.project().dirty);
        }
        outcome
    }

    fn watched_files_changed(&self) -> bool {
        for (path, previous) in &self.watched_files {
            match file_mtime(path) {
                Some(current) => {
                    if &current != previous {
                        return true;
                    }
                }
                None => return true,
            }
        }
        // Also detect newly added plugins.
        let paths = package_paths();
        for root in paths.plugin_roots {
            let current = discover_in_root(&root);
            // If discovery finds paths we do not watch yet, reload.
            for candidate in current {
                if !self
                    .watched_files
                    .iter()
                    .any(|(watched, _)| watched == &candidate.path)
                    && !self
                        .plugins
                        .iter()
                        .any(|plugin| plugin.path == candidate.path)
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn reload(
        &mut self,
        model: &mut ApplicationModel,
        settings: &mut SettingsStore,
        catalog: &EffectCatalog,
        defaults: &ProjectDefaults,
        max_layers: i32,
        default_duration: i32,
    ) -> ModTickOutcome {
        let base = base_snapshot(model);
        self.dispatch_and_queue(ScriptHook::Unload, settings, &base);
        let mut outcome = self.apply_pending(
            model,
            settings,
            catalog,
            defaults,
            max_layers,
            default_duration,
        );
        self.permissions = permissions_from(settings);
        self.plugins.clear();
        self.pending.clear();
        self.discover_and_load(settings, &base_snapshot(model));
        let fresh = base_snapshot(model);
        self.dispatch_and_queue(ScriptHook::Load, settings, &fresh);
        let applied = self.apply_pending(
            model,
            settings,
            catalog,
            defaults,
            max_layers,
            default_duration,
        );
        outcome.applied_commands += applied.applied_commands;
        outcome.diagnostics.extend(applied.diagnostics);
        outcome.model_changed |= applied.model_changed;
        outcome.settings_changed |= applied.settings_changed;
        outcome.reloaded = true;
        self.last_project_key = Some(project_key(model));
        self.last_clips_signature = clips_signature(model);
        outcome
    }
}

fn hook_clone(hook: &ScriptHook) -> ScriptHook {
    match hook {
        ScriptHook::Load => ScriptHook::Load,
        ScriptHook::Unload => ScriptHook::Unload,
        ScriptHook::Update => ScriptHook::Update,
        ScriptHook::ProjectOpen(path) => ScriptHook::ProjectOpen(path.clone()),
        ScriptHook::ProjectSave(path) => ScriptHook::ProjectSave(path.clone()),
        ScriptHook::ClipChange => ScriptHook::ClipChange,
    }
}

struct DiscoveredCandidate {
    id: String,
    expected_id: String,
    manifest: aviqtl_rust_core::api::ScriptPluginManifest,
    path: PathBuf,
    manifest_path: Option<PathBuf>,
    single_file: bool,
}

fn discover_in_root(root: &Path) -> Vec<DiscoveredCandidate> {
    let mut candidates = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return candidates;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("lua") {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            if file_name.is_empty() {
                continue;
            }
            let id = format!("file:{file_name}");
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(&file_name)
                .to_owned();
            let manifest = aviqtl_rust_core::api::ScriptPluginManifest {
                id: id.clone(),
                name: stem,
                version: "file".to_owned(),
                author: String::new(),
                description: String::new(),
                min_app_version: String::new(),
            };
            candidates.push(DiscoveredCandidate {
                id: id.clone(),
                expected_id: id,
                manifest,
                path,
                manifest_path: None,
                single_file: true,
            });
        } else if path.is_dir() {
            let main = path.join("main.lua");
            if !main.is_file() {
                continue;
            }
            let manifest_path = path.join("manifest.lua");
            let manifest = fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|text| parse_script_plugin_manifest(&text))
                .unwrap_or(aviqtl_rust_core::api::ScriptPluginManifest {
                    id: String::new(),
                    name: path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_owned(),
                    version: String::new(),
                    author: String::new(),
                    description: String::new(),
                    min_app_version: String::new(),
                });
            candidates.push(DiscoveredCandidate {
                id: manifest.id.clone(),
                expected_id: String::new(),
                manifest,
                path: main,
                manifest_path: manifest_path.is_file().then_some(manifest_path),
                single_file: false,
            });
        }
    }
    candidates
}

fn validate_candidate(
    manifest: aviqtl_rust_core::api::ScriptPluginManifest,
    single_file: bool,
    expected_id: &str,
    path_identity: &str,
    loaded: &[ScriptPluginIdentity],
) -> Option<aviqtl_rust_core::api::ScriptPluginManifest> {
    let (manifest, status) = validate_script_plugin_manifest(
        manifest,
        single_file,
        expected_id,
        env!("CARGO_PKG_VERSION"),
        path_identity,
        loaded,
    );
    matches!(status, ScriptPluginValidationStatus::Ok).then_some(manifest)
}

fn canonical_identity(path: &Path) -> String {
    fs::canonicalize(path)
        .map(|canonical| canonical.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn read_bounded(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLUGIN_SCRIPT_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.len() as u64 > MAX_PLUGIN_SCRIPT_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn permissions_from(settings: &SettingsStore) -> PluginPermissionState {
    settings
        .value("pluginPermissions")
        .and_then(PluginPermissionState::from_value)
        .unwrap_or_default()
}

fn resolve_parameters(
    settings: &SettingsStore,
    plugin_id: &str,
    single_file: bool,
    script_path: &Path,
    source: &str,
) -> BTreeMap<String, Value> {
    let mut parameters = BTreeMap::new();
    for parameter in inspect_script_metadata(source).parameters {
        let key = format!("plugin_param.{plugin_id}.{}", parameter.var_name);
        let mut saved = settings.value(&key).cloned();
        if saved.is_none()
            && single_file
            && let Some(file_name) = script_path.file_name().and_then(|name| name.to_str())
        {
            let legacy = format!("plugin_param.single.{file_name}.{}", parameter.var_name);
            saved = settings.value(&legacy).cloned();
        }
        parameters.insert(parameter.var_name, saved.unwrap_or(parameter.default_value));
    }
    parameters
}

fn snapshot_for_plugin(
    base: &ScriptHostSnapshot,
    settings: &SettingsStore,
    plugin_id: &str,
) -> ScriptHostSnapshot {
    let mut snapshot = base.clone();
    let prefix = format!("plugin.{plugin_id}.");
    let mut scoped = BTreeMap::new();
    if let Some(Value::Object(_)) = settings.value("pluginPermissions") {
        // Permissions live under a different key; ignore here.
    }
    // SettingsStore only exposes typed accessors plus snapshot(), so collect
    // scoped keys from the full snapshot.
    for (key, value) in settings.snapshot() {
        if let Some(name) = key.strip_prefix(&prefix)
            && let Value::String(text) = value
        {
            scoped.insert(name.to_owned(), text);
        }
    }
    snapshot.plugin_settings = scoped;
    snapshot
}

fn base_snapshot(model: &ApplicationModel) -> ScriptHostSnapshot {
    let Some(workspace) = model.current_workspace() else {
        return ScriptHostSnapshot::default();
    };
    let settings = &workspace.document().settings;
    let clips = workspace
        .document()
        .clips
        .iter()
        .filter(|clip| clip.scene_id == workspace.selected_scene())
        .map(|clip| ScriptClipSnapshot {
            id: clip.id,
            clip_type: clip.clip_type.clone(),
            layer: clip.layer,
            start_frame: clip.start,
            duration: clip.duration,
        })
        .collect();
    ScriptHostSnapshot {
        current_frame: workspace.playhead(),
        is_playing: workspace.is_playing(),
        project_width: settings.width,
        project_height: settings.height,
        project_fps: settings.fps,
        clips,
        plugin_settings: BTreeMap::new(),
    }
}

fn project_key(model: &ApplicationModel) -> (Option<u64>, Option<PathBuf>, u64) {
    let instance = model.current_project_instance_id();
    let path = model
        .current_workspace()
        .and_then(|workspace| workspace.project().path.clone());
    let revision = model
        .current_workspace()
        .map(|workspace| workspace.document_revision())
        .unwrap_or(0);
    (instance, path, revision)
}

fn clips_signature(model: &ApplicationModel) -> u64 {
    let Some(workspace) = model.current_workspace() else {
        return 0;
    };
    let mut hasher = DefaultHasher::new();
    workspace.selected_scene().hash(&mut hasher);
    let mut clips = workspace
        .document()
        .clips
        .iter()
        .filter(|clip| clip.scene_id == workspace.selected_scene())
        .collect::<Vec<_>>();
    clips.sort_by_key(|clip| clip.id);
    for clip in clips {
        clip.id.hash(&mut hasher);
        clip.clip_type.hash(&mut hasher);
        clip.layer.hash(&mut hasher);
        clip.start.hash(&mut hasher);
        clip.duration.hash(&mut hasher);
        clip.effects.len().hash(&mut hasher);
    }
    hasher.finish()
}

struct CommandEffects {
    model_changed: bool,
    settings_changed: bool,
}

#[allow(clippy::too_many_arguments)]
fn apply_command(
    model: &mut ApplicationModel,
    settings: &mut SettingsStore,
    catalog: &EffectCatalog,
    defaults: &ProjectDefaults,
    max_layers: i32,
    default_duration: i32,
    plugin_id: &str,
    command: ScriptHostCommand,
) -> Result<CommandEffects, String> {
    let mut effects = CommandEffects {
        model_changed: false,
        settings_changed: false,
    };
    match command {
        ScriptHostCommand::Log(message) => {
            eprintln!("[{plugin_id}] {message}");
        }
        ScriptHostCommand::TransportPlay => {
            if let Some(workspace) = model.current_workspace_mut()
                && !workspace.is_playing()
            {
                workspace.toggle_playback();
                effects.model_changed = true;
            }
        }
        ScriptHostCommand::TransportPause => {
            if let Some(workspace) = model.current_workspace_mut()
                && workspace.is_playing()
            {
                workspace.pause_playback();
                effects.model_changed = true;
            }
        }
        ScriptHostCommand::TransportToggle => {
            if let Some(workspace) = model.current_workspace_mut() {
                workspace.toggle_playback();
                effects.model_changed = true;
            }
        }
        ScriptHostCommand::TransportSeek(frame) => {
            if let Some(workspace) = model.current_workspace_mut() {
                workspace.seek(frame);
                effects.model_changed = true;
            }
        }
        ScriptHostCommand::ClipCreate {
            clip_type,
            start_frame,
            layer,
        } => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.insert_catalog_object_at(
                &clip_type,
                start_frame,
                layer,
                default_duration,
                catalog,
            ) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::ClipDelete(clip_id) => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.remove_clip(clip_id) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::ClipUpdate {
            clip_id,
            layer,
            start_frame,
            duration,
        } => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.update_clip_geometry(clip_id, layer, start_frame, duration) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::ClipSelect(clip_id) => {
            if let Some(workspace) = model.current_workspace_mut() {
                workspace.click_clip(clip_id, false);
                effects.model_changed = true;
            }
        }
        ScriptHostCommand::ClipSplit { clip_id, frame } => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.split_clip(clip_id, frame) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::ClipCopy(clip_id) => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.copy_clip(clip_id) {
                return Err(format!("clip {clip_id} does not exist"));
            }
        }
        ScriptHostCommand::ClipCut(clip_id) => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.cut_clip(clip_id) {
                return Err(format!("clip {clip_id} does not exist"));
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::ClipPaste { frame, layer } => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if workspace.paste_clips_at(frame, layer, max_layers).is_none() {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::EffectAdd {
            clip_id,
            effect_type,
        } => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.add_effect_to_clip(clip_id, catalog, &effect_type) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::EffectRemove {
            clip_id,
            effect_index,
        } => {
            let effect_index = usize::try_from(effect_index)
                .map_err(|_| "effect index is out of range".to_owned())?;
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.remove_effect_from_clip(clip_id, effect_index) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::EffectSetParameter {
            clip_id,
            effect_index,
            name,
            value,
        } => {
            let effect_index = usize::try_from(effect_index)
                .map_err(|_| "effect index is out of range".to_owned())?;
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.set_effect_param_for_clip(clip_id, effect_index, &name, value) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::ProjectSave(path) => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if path.trim().is_empty() {
                return Err("project save path is empty".to_owned());
            }
            workspace.project_mut().save_as(Path::new(&path))?;
            effects.model_changed = true;
        }
        ScriptHostCommand::ProjectLoad(path) => {
            if path.trim().is_empty() {
                return Err("project load path is empty".to_owned());
            }
            model.open_project(Path::new(&path))?;
            effects.model_changed = true;
        }
        ScriptHostCommand::Undo => {
            if let Some(workspace) = model.current_workspace_mut() {
                if !workspace.undo() {
                    return Err("nothing to undo".to_owned());
                }
                effects.model_changed = true;
            }
        }
        ScriptHostCommand::Redo => {
            if let Some(workspace) = model.current_workspace_mut() {
                if !workspace.redo() {
                    return Err("nothing to redo".to_owned());
                }
                effects.model_changed = true;
            }
        }
        ScriptHostCommand::SettingsSet { key, value } => {
            let scoped = format!("plugin.{plugin_id}.{key}");
            let mut replacement = settings.snapshot();
            replacement.insert(scoped, Value::String(value));
            settings
                .apply(replacement)
                .map_err(|error| format!("settings save failed: {error}"))?;
            effects.settings_changed = true;
        }
        ScriptHostCommand::SceneCreate(name) => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if name.trim().is_empty() {
                return Err("scene name is empty".to_owned());
            }
            let project = workspace.project_settings();
            let scene_defaults = workspace
                .selected_scene_settings()
                .map(|scene| crate::SceneSettingsInput {
                    name: name.clone(),
                    width: scene.width,
                    height: scene.height,
                    fps: scene.fps,
                    duration: scene.duration,
                    grid_mode: scene.grid_mode,
                    grid_bpm: scene.grid_bpm,
                    grid_offset: scene.grid_offset,
                    grid_interval: scene.grid_interval,
                    grid_subdivision: scene.grid_subdivision,
                    enable_snap: scene.enable_snap,
                    magnetic_snap_range: scene.magnetic_snap_range,
                })
                .unwrap_or(crate::SceneSettingsInput {
                    name: name.clone(),
                    width: project.width,
                    height: project.height,
                    fps: project.fps,
                    duration: 300,
                    grid_mode: "Auto".to_owned(),
                    grid_bpm: 120.0,
                    grid_offset: 0.0,
                    grid_interval: 10,
                    grid_subdivision: 4,
                    enable_snap: true,
                    magnetic_snap_range: 10,
                });
            if workspace.create_scene(*defaults, scene_defaults).is_none() {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::SceneRemove(scene_id) => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.remove_scene(scene_id) {
                return Err(workspace.status().to_owned());
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::SceneSwitch(scene_id) => {
            let Some(workspace) = model.current_workspace_mut() else {
                return Err("no project is open".to_owned());
            };
            if !workspace.switch_scene(scene_id) {
                return Err(format!("scene {scene_id} does not exist"));
            }
            effects.model_changed = true;
        }
        ScriptHostCommand::CommandBeginGroup(_) => {
            if let Some(workspace) = model.current_workspace_mut() {
                workspace.begin_undo_group();
            }
        }
        ScriptHostCommand::CommandEndGroup => {
            if let Some(workspace) = model.current_workspace_mut() {
                workspace.end_undo_group();
            }
        }
    }
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_io::ProjectDefaults;
    use serde_json::{Map, json};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_settings(path: PathBuf) -> SettingsStore {
        SettingsStore::load_from(path, Map::new()).0
    }

    fn temporary_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "aviqtl-mod-host-{}-{nanos}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn bundled_examples_load_and_run_load_hooks_without_permissions() {
        let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("plugins");
        for directory in [
            "example_animation",
            "example_clip_ops",
            "example_project_info",
            "example_transport",
        ] {
            let source = fs::read_to_string(plugin_root.join(directory).join("main.lua"))
                .expect("bundled example script remains readable");
            let manifest_source =
                fs::read_to_string(plugin_root.join(directory).join("manifest.lua"))
                    .expect("bundled example manifest remains readable");
            let manifest = parse_script_plugin_manifest(&manifest_source)
                .expect("bundled example manifest remains valid");
            let parameters = inspect_script_metadata(&source)
                .parameters
                .into_iter()
                .map(|parameter| (parameter.var_name, parameter.default_value))
                .collect();
            let permissions = PluginPermissionState::default();
            let (mut runtime, initial) = ScriptRuntime::load(
                manifest.id,
                &source,
                &format!("{directory}/main.lua"),
                &parameters,
                &permissions,
                ScriptHostSnapshot::default(),
            )
            .expect("bundled example loads with the default permission set");
            assert!(
                initial.diagnostics.is_empty(),
                "{directory}: {:?}",
                initial.diagnostics
            );
            let output = runtime.dispatch(
                ScriptHook::Load,
                &permissions,
                ScriptHostSnapshot::default(),
            );
            assert!(
                output.diagnostics.is_empty(),
                "{directory}: {:?}",
                output.diagnostics
            );
        }
    }

    #[test]
    fn update_hook_commands_drive_the_workspace() {
        let settings_path = temporary_path("settings.json");
        let settings = test_settings(settings_path.clone());
        let snapshot = ScriptHostSnapshot::default();
        let source = r#"
            function AviQtlUpdateHook()
                aviqtl.transport.seek(1)
                aviqtl.log("ticked")
            end
        "#;
        let mut permissions = PluginPermissionState::default();
        permissions.grant_all("script.test");
        let (mut runtime, _) = ScriptRuntime::load(
            "script.test",
            source,
            "test.lua",
            &BTreeMap::new(),
            &permissions,
            snapshot,
        )
        .expect("test runtime loads");
        let execution = runtime.dispatch(
            ScriptHook::Update,
            &permissions,
            ScriptHostSnapshot::default(),
        );
        assert_eq!(
            execution.commands,
            vec![
                ScriptHostCommand::TransportSeek(1),
                ScriptHostCommand::Log("ticked".to_owned()),
            ]
        );

        let mut model = ApplicationModel::default();
        model.create_project(ProjectDefaults::default());
        let mut settings = settings;
        let catalog = EffectCatalog::load().0;
        let mut host = ModHost::load(&settings);
        host.plugins.clear();
        host.plugins.push(LoadedPlugin {
            id: "script.test".to_owned(),
            path: PathBuf::from("test.lua"),
            parameters: BTreeMap::new(),
            runtime,
        });
        host.permissions = permissions;
        host.pending.push(execution);
        let outcome = host.apply_pending(
            &mut model,
            &mut settings,
            &catalog,
            &ProjectDefaults::default(),
            128,
            100,
        );
        assert_eq!(outcome.applied_commands, 2);
        assert_eq!(model.current_workspace().expect("workspace").playhead(), 1);
        let _ = fs::remove_file(settings_path);
    }

    #[test]
    fn undo_group_commands_combine_into_one_history_entry() {
        let settings_path = temporary_path("group-settings.json");
        let mut settings = test_settings(settings_path.clone());
        let mut model = ApplicationModel::default();
        model.create_project(ProjectDefaults::default());
        let catalog = EffectCatalog::load().0;
        let mut host = ModHost {
            plugins: Vec::new(),
            permissions: PluginPermissionState::default(),
            pending: vec![ScriptExecution {
                plugin_id: "script.test".to_owned(),
                commands: vec![
                    ScriptHostCommand::CommandBeginGroup("group".to_owned()),
                    ScriptHostCommand::ClipCreate {
                        clip_type: "rect".to_owned(),
                        start_frame: 0,
                        layer: 0,
                    },
                    ScriptHostCommand::ClipCreate {
                        clip_type: "rect".to_owned(),
                        start_frame: 120,
                        layer: 0,
                    },
                    ScriptHostCommand::CommandEndGroup,
                ],
                diagnostics: Vec::new(),
            }],
            diagnostics: Vec::new(),
            watched_files: Vec::new(),
            last_update: Instant::now(),
            last_hot_check: Instant::now(),
            last_project_key: None,
            last_clips_signature: 0,
            last_dirty: false,
            dispatching: false,
        };
        let outcome = host.apply_pending(
            &mut model,
            &mut settings,
            &catalog,
            &ProjectDefaults::default(),
            128,
            100,
        );
        assert_eq!(outcome.applied_commands, 4);
        let workspace = model.current_workspace().expect("workspace");
        assert_eq!(workspace.document().clips.len(), 2);
        let workspace = model.current_workspace_mut().expect("workspace");
        assert!(workspace.undo());
        assert_eq!(workspace.document().clips.len(), 0);
        let _ = fs::remove_file(settings_path);
        let _ = json!(null);
    }
}
