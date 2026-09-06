use crate::project_io::write_atomic;
use crate::settings::application_data_root;
use aviqtl_rust_core::api::{
    EffectPreset, build_effect_preset, parse_effect_preset, preset_name_is_safe,
};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct PresetStore {
    root: PathBuf,
}

impl PresetStore {
    pub fn load() -> Self {
        Self {
            root: default_preset_root(),
        }
    }

    #[cfg(test)]
    fn from_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn names(&self, effect_id: &str) -> Vec<String> {
        let Ok(directory) = self.effect_directory(effect_id, false) else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut names = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("json"))
                .then(|| path.file_stem()?.to_str().map(str::to_owned))
                .flatten()
            })
            .filter(|name| preset_name_is_safe(name))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn load_preset(&self, effect_id: &str, name: &str) -> Result<EffectPreset, String> {
        let path = self.preset_path(effect_id, name, false)?;
        let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        parse_effect_preset(&bytes, effect_id, name).map_err(|error| error.to_string())
    }

    pub fn save(
        &self,
        effect_id: &str,
        name: &str,
        params: Map<String, Value>,
        keyframes: Map<String, Value>,
        enabled: bool,
    ) -> Result<(), String> {
        let path = self.preset_path(effect_id, name, true)?;
        let bytes = build_effect_preset(effect_id, name, enabled, params, keyframes)
            .map_err(|error| error.to_string())?;
        write_atomic(&path, &bytes).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn delete(&self, effect_id: &str, name: &str) -> Result<(), String> {
        let path = self.preset_path(effect_id, name, false)?;
        fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))
    }

    fn preset_path(
        &self,
        effect_id: &str,
        name: &str,
        create_directory: bool,
    ) -> Result<PathBuf, String> {
        if !preset_name_is_safe(name) {
            return Err("preset name is not a safe path component".to_owned());
        }
        Ok(self
            .effect_directory(effect_id, create_directory)?
            .join(format!("{name}.json")))
    }

    fn effect_directory(&self, effect_id: &str, create: bool) -> Result<PathBuf, String> {
        if !preset_name_is_safe(effect_id) {
            return Err("effect id is not a safe path component".to_owned());
        }
        if create {
            fs::create_dir_all(&self.root)
                .map_err(|error| format!("{}: {error}", self.root.display()))?;
        }
        let directory = self.root.join(effect_id);
        if directory.exists() {
            let canonical_root = fs::canonicalize(&self.root)
                .map_err(|error| format!("{}: {error}", self.root.display()))?;
            let canonical_directory = fs::canonicalize(&directory)
                .map_err(|error| format!("{}: {error}", directory.display()))?;
            if !canonical_directory.starts_with(&canonical_root) {
                return Err("preset directory escapes the preset root".to_owned());
            }
        } else if create {
            fs::create_dir(&directory)
                .map_err(|error| format!("{}: {error}", directory.display()))?;
        }
        Ok(directory)
    }
}

fn default_preset_root() -> PathBuf {
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let portable = directory.join("presets");
        if portable.exists() || directory_is_writable(directory) {
            return portable;
        }
    }
    application_data_root().join("presets")
}

fn directory_is_writable(directory: &Path) -> bool {
    fs::metadata(directory)
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "aviqtl-egui-presets-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn save_load_list_delete_and_identity_checks_match_qt() {
        let root = temporary_root();
        let store = PresetStore::from_root(root.clone());
        store
            .save(
                "blur",
                "Warm",
                serde_json::json!({"size": 5})
                    .as_object()
                    .cloned()
                    .expect("params"),
                Map::new(),
                true,
            )
            .expect("save preset");
        assert_eq!(store.names("blur"), ["Warm"]);
        assert_eq!(
            store
                .load_preset("blur", "Warm")
                .expect("load preset")
                .params["size"],
            5
        );
        assert!(store.load_preset("mosaic", "Warm").is_err());
        store.delete("blur", "Warm").expect("delete preset");
        assert!(store.names("blur").is_empty());
        fs::remove_dir_all(root).expect("remove temporary preset root");
    }
}
