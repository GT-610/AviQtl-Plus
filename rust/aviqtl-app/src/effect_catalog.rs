use aviqtl_rust_core::api::{EffectDocument, EffectMetadata, parse_effect_metadata};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_EFFECT_DEFINITION_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default)]
pub struct EffectCatalog {
    entries: Vec<EffectMetadata>,
    indices: BTreeMap<String, usize>,
}

impl EffectCatalog {
    pub fn load() -> (Self, String) {
        let roots = metadata_roots();
        let mut catalog = Self::default();
        let mut loaded_files = BTreeSet::new();
        for root in roots {
            for path in json_files(&root) {
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
                if entry.source.is_empty() {
                    entry.source = "built-in".to_owned();
                }
                if entry.source_path.is_empty() {
                    entry.source_path = path.display().to_string();
                }
                catalog.register(entry);
            }
        }
        let status = if catalog.entries.is_empty() {
            "Effect catalog · no definitions found".to_owned()
        } else {
            format!("Effect catalog · {} definitions", catalog.entries.len())
        };
        (catalog, status)
    }

    fn register(&mut self, entry: EffectMetadata) {
        if let Some(index) = self.indices.get(&entry.id).copied() {
            self.entries[index] = entry;
        } else {
            let index = self.entries.len();
            self.indices.insert(entry.id.clone(), index);
            self.entries.push(entry);
        }
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

fn metadata_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        roots.push(directory.join("effects"));
        roots.push(directory.join("objects"));
        roots.push(directory.join("../Resources/effects"));
        roots.push(directory.join("../Resources/objects"));
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui/qml/effects"));
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui/qml/objects"));
    roots
}

fn json_files(root: &Path) -> Vec<PathBuf> {
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
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
        }
    }

    #[test]
    fn query_matches_the_qt_catalog_fields_and_parent_categories() {
        let mut catalog = EffectCatalog::default();
        catalog.register(entry("soft_blur", "Soft Blur", &["Blur/Gaussian"]));
        catalog.register(entry("noise", "Noise", &["Stylize"]));

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
        assert_eq!(catalog.entries.len(), 61);
        assert_eq!(catalog.find("blur").expect("blur").kind, "effect");
        assert_eq!(catalog.find("rect").expect("rect").kind, "object");
        assert_eq!(
            catalog.effect_document("blur").expect("effect").params["quality"],
            1
        );
        assert!(catalog.effect_document("rect").is_none());
    }
}
