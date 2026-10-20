//! Import check against a real AviUtl installation.
//!
//! Synthetic fixtures cover the code paths an object file can take, but only real files
//! exercise the combinations AviUtl actually writes: the object types mixed together, the
//! filter stacks, the code pages, the parameter spellings. This test imports every shipped
//! `.exo` and checks that nothing was silently lost on the way in.
//!
//! It is ignored by default because it needs an AviUtl package that is not part of the
//! repository. Point `AVIUTL_SAMPLES` at the package root (the directory holding
//! `exedit.auf`) and run:
//!
//! ```sh
//! AVIUTL_SAMPLES=/path/to/AviUtl_CN set
//! cargo test -p aviqtl-app --test exo_samples -- --ignored --nocapture
//! ```

use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::exo_import::{ImportPlacement, PLACEHOLDER_ID, plan_import};
use aviqtl_rust_core::api::{ProjectDocument, ProjectSettings, SceneDocument};
use aviqtl_rust_core::exo::{ExoFile, ExoObject};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Collects every `.exo` file below `root`.
fn object_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exo"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn scene_for(file: &ExoFile) -> SceneDocument {
    SceneDocument {
        id: 1,
        name: "Root".to_owned(),
        width: file.header.width,
        height: file.header.height,
        fps: file.header.rate,
        start: 0,
        duration: file.header.length.max(1),
        nested_duration: 0,
        locked_layers: Vec::new(),
        hidden_layers: Vec::new(),
        grid_mode: "none".to_owned(),
        grid_bpm: 120.0,
        grid_offset: 0.0,
        grid_interval: 10,
        grid_subdivision: 4,
        enable_snap: true,
        magnetic_snap_range: 10,
        extra: Default::default(),
    }
}

fn empty_project(file: &ExoFile, scene: &SceneDocument) -> ProjectDocument {
    ProjectDocument {
        version: 3,
        settings: ProjectSettings {
            width: file.header.width,
            height: file.header.height,
            fps: file.header.rate,
            sample_rate: file.header.audio_rate,
            extra: Default::default(),
        },
        scenes: vec![scene.clone()],
        clips: Vec::new(),
        extra: Default::default(),
    }
}

/// What went wrong for one file, or nothing when the import held up.
fn check_import(file: &ExoFile, catalog: &EffectCatalog) -> Vec<String> {
    let mut problems = Vec::new();
    let scene = scene_for(file);
    // An empty scene means the import has nothing to collide with, so every object keeps the
    // placement the file gave it and the two lists line up one to one.
    let outcome = plan_import(
        file,
        &scene,
        &empty_project(file, &scene),
        catalog,
        1,
        ImportPlacement::default(),
    );

    let expected = file
        .objects
        .iter()
        .filter(|object| object.object_filter().is_some())
        .collect::<Vec<&ExoObject>>();
    if expected.len() != outcome.clips.len() {
        problems.push(format!(
            "{} object(s) with a body became {} clip(s)",
            expected.len(),
            outcome.clips.len()
        ));
    }
    if outcome.report.objects != outcome.clips.len() {
        problems.push(format!(
            "the report counted {} object(s) for {} clip(s)",
            outcome.report.objects,
            outcome.clips.len()
        ));
    }
    if outcome.report.translated + outcome.report.placeholders == 0 && !outcome.clips.is_empty() {
        problems.push("the import reported nothing about what it did".to_owned());
    }

    for (index, (object, clip)) in expected.iter().zip(&outcome.clips).enumerate() {
        // ExEdit frames and layers are 1-based on disk.
        let expected_start = object.start.saturating_sub(1);
        let expected_layer = object.layer.saturating_sub(1);
        if clip.start != expected_start {
            problems.push(format!(
                "[{index}] start {expected_start} -> {}",
                clip.start
            ));
        }
        if clip.duration != object.duration().max(1) {
            problems.push(format!(
                "[{index}] duration {} -> {}",
                object.duration().max(1),
                clip.duration
            ));
        }
        if clip.layer != expected_layer {
            problems.push(format!(
                "[{index}] layer {expected_layer} -> {}",
                clip.layer
            ));
        }
        if clip.clip_type.is_empty() {
            problems.push(format!("[{index}] the clip has no type"));
        }
        if clip.clip_by_upper_object != object.overlay {
            problems.push(format!("[{index}] overlay flag changed"));
        }
        if clip.effects.is_empty() {
            problems.push(format!("[{index}] the clip has no effects"));
        }
        // Every preserved entry keeps enough to be recognised: its name, and its script when
        // it had one. That is the promise that makes a partial translation acceptable.
        for effect in &clip.effects {
            if effect.id != PLACEHOLDER_ID {
                continue;
            }
            let named = effect
                .params
                .get("exoDisplayName")
                .and_then(Value::as_str)
                .is_some_and(|name| !name.is_empty());
            if !named {
                problems.push(format!("[{index}] a placeholder lost its name"));
            }
            if effect.enabled {
                problems.push(format!("[{index}] a placeholder is enabled"));
            }
        }
    }
    problems
}

#[test]
#[ignore = "needs an AviUtl package; set AVIUTL_SAMPLES to its root"]
fn shipped_object_files_import_without_losing_objects() {
    let root = std::env::var("AVIUTL_SAMPLES")
        .map(PathBuf::from)
        .expect("set AVIUTL_SAMPLES to the AviUtl package root");
    let files = object_files(&root);
    assert!(!files.is_empty(), "no .exo files under {}", root.display());

    let (catalog, _) = EffectCatalog::load();
    let mut failures = Vec::new();
    let mut total = 0;
    for path in &files {
        let bytes = std::fs::read(path).expect("object file is readable");
        let file = ExoFile::parse(&bytes).expect("object file parses");
        total += file.objects.len();
        for problem in check_import(&file, &catalog) {
            failures.push(format!("{}: {problem}", path.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} problem(s) across {} file(s), {} object(s):\n{}",
        failures.len(),
        files.len(),
        total,
        failures.join("\n")
    );
}
