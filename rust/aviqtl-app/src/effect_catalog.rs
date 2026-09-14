use crate::settings::package_paths;
use aviqtl_render::{NativeRenderDefinition, validate_native_shader};
use aviqtl_rust_core::api::{EffectDocument, EffectMetadata, parse_effect_metadata};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_EFFECT_DEFINITION_BYTES: u64 = 1024 * 1024;
const MAX_NATIVE_SHADER_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default)]
pub struct EffectCatalog {
    entries: Vec<EffectMetadata>,
    indices: BTreeMap<String, usize>,
    native_definitions: BTreeMap<String, NativeRenderDefinition>,
}

impl EffectCatalog {
    pub fn load() -> (Self, String) {
        let (roots, user_package_roots) = metadata_roots();
        Self::load_from_roots(roots, user_package_roots)
    }

    fn load_from_roots(
        roots: Vec<PathBuf>,
        user_package_roots: Vec<(PathBuf, &'static str)>,
    ) -> (Self, String) {
        let mut catalog = Self::default();
        let mut loaded_files = BTreeSet::new();
        for root in roots {
            for path in json_files(&root) {
                let user_package =
                    user_package_roots
                        .iter()
                        .find_map(|(package_root, package_type)| {
                            user_package_context(&path, package_root).map(
                                |(package_id, directory)| (package_id, directory, *package_type),
                            )
                        });
                let identity = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if !loaded_files.insert(identity) {
                    continue;
                }
                let Ok(metadata) = fs::metadata(&path) else {
                    continue;
                };
                if metadata.len() > MAX_EFFECT_DEFINITION_BYTES {
                    continue;
                }
                let Ok(bytes) = fs::read(&path) else {
                    continue;
                };
                let Some(mut entry) = parse_effect_metadata(&bytes) else {
                    continue;
                };
                let native_definition =
                    if let Some((package_id, package_directory, package_type)) = user_package {
                        if entry.kind != package_type {
                            continue;
                        }
                        if entry.package_id.is_empty() {
                            entry.package_id = package_id.clone();
                        } else if entry.package_id != package_id {
                            continue;
                        }
                        let Some(definition) =
                            load_native_definition(&entry, &path, &package_directory)
                        else {
                            continue;
                        };
                        entry.source = "package".to_owned();
                        entry.source_path = path.display().to_string();
                        Some(definition)
                    } else {
                        entry.runtime.as_ref().and_then(|_| {
                            let package_directory = path.parent()?;
                            load_native_definition(&entry, &path, package_directory)
                        })
                    };
                if entry.source.is_empty() {
                    entry.source = if native_definition.is_some() {
                        "package".to_owned()
                    } else {
                        "built-in".to_owned()
                    };
                }
                if entry.source_path.is_empty() {
                    entry.source_path = path.display().to_string();
                }
                catalog.register(entry, native_definition);
            }
        }
        let status = if catalog.entries.is_empty() {
            "Effect catalog · no definitions found".to_owned()
        } else {
            format!("Effect catalog · {} definitions", catalog.entries.len())
        };
        (catalog, status)
    }

    fn register(
        &mut self,
        entry: EffectMetadata,
        native_definition: Option<NativeRenderDefinition>,
    ) {
        if let Some(definition) = native_definition {
            self.native_definitions.insert(entry.id.clone(), definition);
        } else {
            self.native_definitions.remove(&entry.id);
        }
        if let Some(index) = self.indices.get(&entry.id).copied() {
            self.entries[index] = entry;
        } else {
            let index = self.entries.len();
            self.indices.insert(entry.id.clone(), index);
            self.entries.push(entry);
        }
    }

    pub fn native_definitions(&self) -> Vec<NativeRenderDefinition> {
        self.native_definitions.values().cloned().collect()
    }

    pub fn find(&self, id: &str) -> Option<&EffectMetadata> {
        self.indices
            .get(id)
            .and_then(|index| self.entries.get(*index))
    }

    pub fn entries(&self, kind: &str) -> impl Iterator<Item = &EffectMetadata> {
        self.entries.iter().filter(move |entry| entry.kind == kind)
    }

    pub fn query<'a>(&'a self, kind: &str, query: &str, category: &str) -> Vec<&'a EffectMetadata> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .filter(|entry| category_matches(&entry.categories, category))
            .filter(|entry| metadata_matches(entry, query))
            .collect()
    }

    pub fn categories(&self, kind: &str) -> Vec<String> {
        let mut categories = BTreeSet::new();
        for entry in self.entries.iter().filter(|entry| entry.kind == kind) {
            for category in &entry.categories {
                if category.is_empty() {
                    continue;
                }
                categories.insert(category.clone());
                if let Some((parent, _)) = category.split_once('/')
                    && !parent.is_empty()
                {
                    categories.insert(parent.to_owned());
                }
            }
        }
        let mut categories = categories.into_iter().collect::<Vec<_>>();
        categories.sort_by_key(|category| category.to_lowercase());
        categories
    }

    pub fn effect_document(&self, id: &str) -> Option<EffectDocument> {
        let entry = self.find(id)?;
        (entry.kind == "effect").then(|| EffectDocument {
            id: entry.id.clone(),
            name: entry.name.clone(),
            enabled: true,
            params: entry.params.clone(),
            keyframes: None,
            extra: BTreeMap::new(),
        })
    }
}

fn metadata_matches(metadata: &EffectMetadata, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || [
            metadata.name.as_str(),
            metadata.id.as_str(),
            metadata.version.as_str(),
            if metadata.source.is_empty() {
                "built-in"
            } else {
                metadata.source.as_str()
            },
            metadata.package_id.as_str(),
            metadata.source_path.as_str(),
        ]
        .into_iter()
        .chain(metadata.categories.iter().map(String::as_str))
        .any(|field| field.to_lowercase().contains(&query))
}

fn category_matches(categories: &[String], category: &str) -> bool {
    if category.is_empty() {
        return true;
    }
    let category = category.to_lowercase();
    let prefix = format!("{category}/");
    categories.iter().any(|candidate| {
        let candidate = candidate.to_lowercase();
        candidate == category || candidate.starts_with(&prefix)
    })
}

fn metadata_roots() -> (Vec<PathBuf>, Vec<(PathBuf, &'static str)>) {
    let paths = package_paths();
    let user_package_roots = paths
        .package_root
        .parent()
        .map(|root| {
            vec![
                (root.join("effects"), "effect"),
                (root.join("objects"), "object"),
            ]
        })
        .unwrap_or_default();
    let roots = paths
        .effect_roots
        .into_iter()
        .chain(paths.object_roots)
        .collect();
    (roots, user_package_roots)
}

fn user_package_context(path: &Path, package_root: &Path) -> Option<(String, PathBuf)> {
    let relative = path.strip_prefix(package_root).ok()?;
    if relative.components().count() <= 1 {
        return None;
    }
    let package_id = relative
        .components()
        .next()?
        .as_os_str()
        .to_str()?
        .to_owned();
    Some((package_id.clone(), package_root.join(package_id)))
}

fn load_native_definition(
    metadata: &EffectMetadata,
    metadata_path: &Path,
    package_directory: &Path,
) -> Option<NativeRenderDefinition> {
    let runtime = metadata.runtime.as_ref()?;
    if runtime
        .uniforms
        .iter()
        .any(|name| !metadata.params.contains_key(name))
    {
        return None;
    }
    let package_directory = package_directory.canonicalize().ok()?;
    let shader_path = metadata_path.parent()?.join(&runtime.shader);
    let shader_path = shader_path.canonicalize().ok()?;
    if !shader_path.starts_with(&package_directory) {
        return None;
    }
    let file_metadata = fs::metadata(&shader_path).ok()?;
    if !file_metadata.is_file() || file_metadata.len() > MAX_NATIVE_SHADER_BYTES {
        return None;
    }
    let shader_source = fs::read_to_string(shader_path).ok()?;
    validate_native_shader(&shader_source).ok()?;
    Some(NativeRenderDefinition {
        id: metadata.id.clone(),
        kind: metadata.kind.clone(),
        uniforms: runtime.uniforms.clone(),
        shader_source: Arc::from(shader_source),
    })
}

pub(crate) fn validate_native_package_directory(
    package_directory: &Path,
    package_id: &str,
    package_type: &str,
) -> Result<(), String> {
    let mut definitions = 0usize;
    let mut ids = BTreeSet::new();
    for path in json_files(package_directory) {
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        if metadata.len() > MAX_EFFECT_DEFINITION_BYTES {
            return Err(format!(
                "Native package JSON exceeds the size limit at {}.",
                path.display()
            ));
        }
        let bytes = fs::read(&path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let document = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
        let declares_render_entry = document
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .is_some_and(|document| {
                document.contains_key("runtime")
                    || document
                        .get("kind")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|kind| matches!(kind, "effect" | "object"))
            });
        let Some(entry) = parse_effect_metadata(&bytes) else {
            if declares_render_entry {
                return Err(format!(
                    "Native package contains invalid render metadata at {}.",
                    path.display()
                ));
            }
            continue;
        };
        if entry.runtime.is_none() {
            return Err(format!(
                "Native package entry {} does not declare aviqtl-wgsl-v1.",
                entry.id
            ));
        }
        if entry.kind != package_type {
            return Err(format!(
                "Native package entry {} has type {}, expected {package_type}.",
                entry.id, entry.kind
            ));
        }
        if !entry.package_id.is_empty() && entry.package_id != package_id {
            return Err(format!(
                "Native package entry {} belongs to a different package.",
                entry.id
            ));
        }
        if !ids.insert(entry.id.clone()) {
            return Err(format!(
                "Native package contains duplicate entry {}.",
                entry.id
            ));
        }
        load_native_definition(&entry, &path, package_directory).ok_or_else(|| {
            format!(
                "Native package entry {} has an invalid or unreadable WGSL shader.",
                entry.id
            )
        })?;
        definitions += 1;
    }
    if definitions == 0 {
        return Err(format!(
            "{package_type} packages must contain at least one aviqtl-wgsl-v1 entry."
        ));
    }
    Ok(())
}

fn json_files(root: &Path) -> Vec<PathBuf> {
    let mut directories = vec![root.to_path_buf()];
    let mut visited = BTreeSet::new();
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        let canonical = directory.canonicalize().unwrap_or(directory.clone());
        if !visited.insert(canonical) {
            continue;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "aviqtl-native-catalog-{}-{nanos}-{}",
            std::process::id(),
            PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    const PASSTHROUGH_SHADER: &str = r#"
fn aviqtl_effect(
    input_color: vec4<f32>,
    uv: vec2<f32>,
    canvas_size: vec2<f32>,
    time_seconds: f32,
) -> vec4<f32> {
    let amount = aviqtl_parameter(0u).x;
    return input_color + vec4<f32>(uv / max(canvas_size, vec2<f32>(1.0)), time_seconds, amount) * 0.0;
}
"#;

    fn entry(id: &str, name: &str, categories: &[&str]) -> EffectMetadata {
        EffectMetadata {
            id: id.to_owned(),
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            kind: "effect".to_owned(),
            categories: categories.iter().map(|value| (*value).to_owned()).collect(),
            params: json!({"amount": 1}).as_object().cloned().expect("params"),
            ui: json!({"controls": []}).as_object().cloned().expect("ui"),
            source: "built-in".to_owned(),
            package_id: String::new(),
            source_path: String::new(),
            runtime: None,
        }
    }

    #[test]
    fn query_matches_the_qt_catalog_fields_and_parent_categories() {
        let mut catalog = EffectCatalog::default();
        catalog.register(entry("soft_blur", "Soft Blur", &["Blur/Gaussian"]), None);
        catalog.register(entry("noise", "Noise", &["Stylize"]), None);

        assert_eq!(
            catalog.categories("effect"),
            ["Blur", "Blur/Gaussian", "Stylize"]
        );
        assert_eq!(catalog.query("effect", "soft", "")[0].id, "soft_blur");
        assert_eq!(catalog.query("effect", "gauss", "Blur")[0].id, "soft_blur");
        assert!(catalog.query("effect", "noise", "Blur").is_empty());
        assert_eq!(
            catalog.effect_document("soft_blur").expect("effect").params["amount"],
            1
        );
    }

    #[test]
    fn runtime_catalog_loads_built_in_effects_and_objects() {
        let (catalog, _) = EffectCatalog::load();
        assert!(
            catalog.entries.len() >= 60,
            "expected the built-in catalog, found {} entries",
            catalog.entries.len()
        );
        assert_eq!(catalog.find("blur").expect("blur").kind, "effect");
        assert_eq!(catalog.find("rect").expect("rect").kind, "object");
        assert_eq!(
            catalog.effect_document("blur").expect("effect").params["quality"],
            1
        );
        assert!(catalog.effect_document("rect").is_none());
    }

    #[test]
    fn user_package_paths_resolve_their_package_directory() {
        let root = Path::new("C:/AviQtl/effects");
        assert!(user_package_context(Path::new("C:/AviQtl/effects/blur.json"), root).is_none());
        assert_eq!(
            user_package_context(
                Path::new("C:/AviQtl/effects/org.example.blur/definitions/blur.json"),
                root
            ),
            Some((
                "org.example.blur".to_owned(),
                PathBuf::from("C:/AviQtl/effects/org.example.blur")
            ))
        );
    }

    #[test]
    fn native_package_validation_requires_matching_metadata_and_shader() {
        let effect_root = temporary_directory();
        let package = effect_root.join("org.example.native");
        let definitions = package.join("definitions");
        let shaders = definitions.join("shaders");
        fs::create_dir_all(&shaders).expect("package directories create");
        fs::write(shaders.join("main.wgsl"), PASSTHROUGH_SHADER).expect("shader writes");
        fs::write(
            definitions.join("main.json"),
            serde_json::to_vec(&json!({
                "id": "effect.native",
                "name": "Native",
                "version": "1.0.0",
                "kind": "effect",
                "categories": ["Native"],
                "params": {"amount": 0.5},
                "ui": {"controls": []},
                "runtime": {
                    "engine": "aviqtl-wgsl-v1",
                    "shader": "shaders/main.wgsl",
                    "uniforms": ["amount"]
                }
            }))
            .expect("metadata serializes"),
        )
        .expect("metadata writes");

        assert!(
            validate_native_package_directory(&package, "org.example.native", "effect").is_ok()
        );
        let (catalog, _) = EffectCatalog::load_from_roots(
            vec![effect_root.clone()],
            vec![(effect_root.clone(), "effect")],
        );
        let loaded = catalog.find("effect.native").expect("native effect loads");
        assert_eq!(loaded.package_id, "org.example.native");
        assert_eq!(loaded.source, "package");
        assert_eq!(catalog.native_definitions().len(), 1);
        assert!(
            validate_native_package_directory(&package, "org.example.native", "object").is_err()
        );
        fs::write(shaders.join("main.wgsl"), "@fragment fn forbidden() {}")
            .expect("shader replaces");
        assert!(
            validate_native_package_directory(&package, "org.example.native", "effect").is_err()
        );

        fs::remove_dir_all(effect_root).expect("temporary package removes");
    }

    #[test]
    fn native_package_validation_rejects_oversized_json() {
        let root = temporary_directory();
        let package = root.join("org.example.native");
        fs::create_dir_all(&package).expect("package directory creates");
        fs::write(
            package.join("oversized.json"),
            vec![b' '; MAX_EFFECT_DEFINITION_BYTES as usize + 1],
        )
        .expect("oversized metadata writes");

        assert!(
            validate_native_package_directory(&package, "org.example.native", "effect").is_err()
        );

        fs::remove_dir_all(root).expect("temporary package removes");
    }

    #[test]
    fn repository_weather_objects_satisfy_the_native_package_contract() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("effect-packages/weather-objects");

        validate_native_package_directory(&package, "com.aviqtl.objects.weather", "object")
            .expect("weather object package is valid");
    }

    #[test]
    fn repository_color_effects_satisfy_the_native_package_contract() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("effect-packages/color-effects");

        validate_native_package_directory(&package, "com.aviqtl.effects.color", "effect")
            .expect("color effect package is valid");
    }
}
