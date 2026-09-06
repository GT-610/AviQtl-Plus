use crate::project_io::write_atomic;
use aviqtl_rust_core::api::SettingsState;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct SettingsStore {
    state: SettingsState,
    path: PathBuf,
}

impl SettingsStore {
    pub fn load() -> (Self, String) {
        Self::load_from(default_settings_path(), platform_default_settings())
    }

    fn load_from(path: PathBuf, platform_defaults: Map<String, Value>) -> (Self, String) {
        let mut store = Self {
            state: SettingsState::defaults(platform_defaults),
            path,
        };
        let bytes = match fs::read(&store.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (store, "Settings · defaults".to_owned());
            }
            Err(error) => {
                let path = store.path.display().to_string();
                return (store, format!("Settings · {path}: {error}"));
            }
        };
        match store.state.merge_json(&bytes) {
            Ok(migrated) => {
                let status = if migrated {
                    match store.save() {
                        Ok(()) => "Settings · loaded and migrated".to_owned(),
                        Err(error) => format!("Settings · migrated in memory; {error}"),
                    }
                } else {
                    "Settings · loaded".to_owned()
                };
                (store, status)
            }
            Err(error) => {
                let path = store.path.display().to_string();
                (store, format!("Settings · {path}: {error}"))
            }
        }
    }

    pub fn snapshot(&self) -> Map<String, Value> {
        self.state.snapshot()
    }

    pub fn apply(&mut self, replacement: Map<String, Value>) -> Result<(), String> {
        let previous = self.state.snapshot();
        self.state.replace(replacement);
        if let Err(error) = self.save() {
            self.state.replace(previous);
            return Err(error);
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn value(&self, key: &str) -> Option<&Value> {
        self.state.value(key)
    }

    pub fn i32_value(&self, key: &str, fallback: i32) -> i32 {
        self.state.i32_value(key, fallback)
    }

    pub fn f64_value(&self, key: &str, fallback: f64) -> f64 {
        self.state.f64_value(key, fallback)
    }

    pub fn bool_value(&self, key: &str, fallback: bool) -> bool {
        self.state.bool_value(key, fallback)
    }

    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let bytes = self
            .state
            .persistent_json()
            .map_err(|error| error.to_string())?;
        write_atomic(&self.path, &bytes)
            .map_err(|error| format!("{}: {error}", self.path.display()))
    }
}

fn default_settings_path() -> PathBuf {
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let portable = directory.join("aviqtl_settings.json");
        let portable_writable = fs::metadata(&portable)
            .map(|metadata| !metadata.permissions().readonly())
            .unwrap_or_else(|_| {
                fs::metadata(directory)
                    .map(|metadata| !metadata.permissions().readonly())
                    .unwrap_or(false)
            });
        if portable_writable {
            return portable;
        }
    }
    application_data_root().join("settings.json")
}

pub fn application_data_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("AviQtl Plus");
    }
    #[cfg(target_os = "windows")]
    if let Some(local_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_data).join("AviQtl Plus");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data).join("AviQtl Plus");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("AviQtl Plus");
        }
    }
    std::env::temp_dir().join("AviQtl Plus")
}

fn platform_default_settings() -> Map<String, Value> {
    let mut settings = Map::new();
    for (format, environment, defaults) in [
        (
            "LADSPA",
            &["LADSPA_PATH"][..],
            &["/usr/lib/ladspa", "/usr/local/lib/ladspa"][..],
        ),
        (
            "DSSI",
            &["DSSI_PATH"][..],
            &["/usr/lib/dssi", "/usr/local/lib/dssi"][..],
        ),
        (
            "LV2",
            &["LV2_PATH"][..],
            &["/usr/lib/lv2", "/usr/local/lib/lv2"][..],
        ),
        (
            "VST2",
            &["VST_PATH"][..],
            &[
                "/usr/lib/vst",
                "/usr/lib/vst2",
                "/usr/local/lib/vst",
                "/usr/local/lib/vst2",
            ][..],
        ),
        (
            "VST3",
            &["VST3_PATH"][..],
            &["/usr/lib/vst3", "/usr/local/lib/vst3"][..],
        ),
        (
            "CLAP",
            &["CLAP_PATH"][..],
            &["/usr/lib/clap", "/usr/local/lib/clap"][..],
        ),
        (
            "SF2",
            &["SF2_PATH"][..],
            &["/usr/share/soundfonts", "/usr/share/sounds/sf2"][..],
        ),
        ("SFZ", &["SFZ_PATH"][..], &["/usr/share/sounds/sfz"][..]),
        ("JSFX", &[][..], &[][..]),
        ("Effects", &["AVIQTL_EFFECTS_PATH"][..], &[][..]),
        ("Objects", &["AVIQTL_OBJECTS_PATH"][..], &[][..]),
    ] {
        settings.insert(
            format!("pluginPaths{format}"),
            Value::Array(
                default_plugin_paths(&format.to_ascii_lowercase(), environment, defaults)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    settings
}

fn default_plugin_paths(format: &str, environment: &[&str], defaults: &[&str]) -> Vec<String> {
    let mut paths = Vec::new();
    let push_unique = |paths: &mut Vec<String>, path: String| {
        if !paths.contains(&path) {
            paths.push(path);
        }
    };
    for name in environment {
        if let Some(value) = std::env::var_os(name) {
            for path in std::env::split_paths(&value)
                .filter_map(|path| path.to_str().map(ToOwned::to_owned))
            {
                push_unique(&mut paths, path);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        push_unique(
            &mut paths,
            home.join(format!(".{format}"))
                .to_string_lossy()
                .into_owned(),
        );
        #[cfg(target_os = "macos")]
        if let Some(directory) = macos_audio_plugin_directory(format) {
            push_unique(
                &mut paths,
                home.join("Library/Audio/Plug-Ins")
                    .join(directory)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(directory) = macos_audio_plugin_directory(format) {
        push_unique(
            &mut paths,
            Path::new("/Library/Audio/Plug-Ins")
                .join(directory)
                .to_string_lossy()
                .into_owned(),
        );
    }
    push_unique(&mut paths, format.to_owned());
    for path in defaults {
        push_unique(&mut paths, (*path).to_owned());
    }
    paths
}

#[cfg(target_os = "macos")]
fn macos_audio_plugin_directory(format: &str) -> Option<&'static str> {
    match format {
        "lv2" => Some("LV2"),
        "vst2" => Some("VST"),
        "vst3" => Some("VST3"),
        "clap" => Some("CLAP"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMPORARY_PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temporary_settings_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = TEMPORARY_PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "aviqtl-egui-settings-{}-{nanos}-{sequence}.json",
            std::process::id(),
        ))
    }

    #[test]
    fn loads_migrates_and_atomically_replaces_settings() {
        let path = temporary_settings_path();
        fs::write(
            &path,
            br#"{"theme":"Light","packageRepositoryUrls":["https://example.invalid"]}"#,
        )
        .expect("fixture writes");
        let (mut store, status) = SettingsStore::load_from(path.clone(), Map::new());
        assert!(status.contains("migrated"));
        assert_eq!(store.value("theme"), Some(&json!("Light")));

        let mut replacement = store.snapshot();
        replacement.insert("backupInterval".to_owned(), json!(12));
        replacement.insert("_runtime".to_owned(), json!(true));
        store.apply(replacement).expect("settings apply");
        assert_eq!(store.i32_value("backupInterval", 0), 12);
        assert!(store.bool_value("_runtime", false));
        let saved: Value = serde_json::from_slice(&fs::read(&path).expect("settings read"))
            .expect("settings parse");
        assert!(saved.get("_runtime").is_none());

        fs::remove_file(path).expect("fixture cleanup");
    }

    #[test]
    fn missing_file_keeps_complete_defaults_without_writing() {
        let path = temporary_settings_path();
        let (store, status) = SettingsStore::load_from(path.clone(), Map::new());
        assert_eq!(status, "Settings · defaults");
        assert_eq!(store.f64_value("previewRenderScale", 0.0), 1.0);
        assert!(!path.exists());
    }
}
