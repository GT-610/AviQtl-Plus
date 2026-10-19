//! Round-trip check against a real AviUtl installation.
//!
//! The importer's guarantee is that an object file comes back with the same objects,
//! filters, and values it went in with. Synthetic fixtures cover the code paths, but only
//! real files exercise the combinations AviUtl actually writes, so this test runs against a
//! directory of them.
//!
//! It is ignored by default because it needs an AviUtl package that is not part of the
//! repository. Point `AVIUTL_SAMPLES` at the package root (the directory holding
//! `exedit.auf`) and run:
//!
//! ```sh
//! AVIUTL_SAMPLES=/path/to/AviUtl_CN set
//! cargo test -p aviqtl-app --test exo_samples -- --ignored --nocapture
//! ```
//!
//! Only differences that change meaning are failures. AviUtl pads its text buffers to a fixed
//! size and writes numbers with the parameter's own decimal count; the exporter writes both
//! canonically, so those differences are expected and reported as counts rather than compared.

use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::exo_export::export_scene;
use aviqtl_app::exo_import::{ImportPlacement, plan_import};
use aviqtl_rust_core::api::{ProjectDocument, ProjectSettings, SceneDocument};
use aviqtl_rust_core::exo::{ExoFile, ExoObject, TrackValue};
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

/// True when two trackbar values mean the same thing.
///
/// Values are compared through the same parser the importer uses, so a field whose precision
/// artifact ExEdit appends (`.0` after a full-precision double) is not mistaken for a change,
/// and a single value compares equal to a range whose ends coincide.
fn same_track(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let parse = |value: &str| -> Option<(f64, f64)> {
        let track = TrackValue::parse(value)?;
        Some((track.start, track.end.unwrap_or(track.start)))
    };
    match (parse(left), parse(right)) {
        (Some((a_start, a_end)), Some((b_start, b_end))) => a_start == b_start && a_end == b_end,
        _ => false,
    }
}

/// True when two hex-encoded UTF-16 buffers hold the same string.
///
/// AviUtl pads the buffer to a fixed size with NUL units and the exporter writes it without
/// the padding, so the bytes differ while the text does not.
fn same_text_buffer(left: &str, right: &str) -> bool {
    let decode = |value: &str| -> Option<String> {
        let bytes = (0..value.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect::<Vec<_>>();
        Some(String::from_utf16_lossy(&units))
    };
    match (decode(left), decode(right)) {
        (Some(a), Some(b)) => a == b,
        _ => left == right,
    }
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

fn compare(original: &ExoFile, reparsed: &ExoFile) -> Vec<String> {
    let mut problems = Vec::new();
    let mut left: Vec<&ExoObject> = original.objects.iter().collect();
    left.sort_by_key(|object| (object.start, object.end, object.layer));
    let mut right: Vec<&ExoObject> = reparsed.objects.iter().collect();
    right.sort_by_key(|object| (object.start, object.end, object.layer));
    if left.len() != right.len() {
        problems.push(format!("object count {} -> {}", left.len(), right.len()));
    }
    for (index, (before, after)) in left.iter().zip(&right).enumerate() {
        if (before.start, before.end, before.layer) != (after.start, after.end, after.layer) {
            problems.push(format!("[{index}] placement changed"));
        }
        if before.filters.len() != after.filters.len() {
            problems.push(format!(
                "[{index}] filter count {} -> {}",
                before.filters.len(),
                after.filters.len()
            ));
            continue;
        }
        for (slot, (a, b)) in before.filters.iter().zip(&after.filters).enumerate() {
            if a.display_name() != b.display_name() {
                problems.push(format!(
                    "[{index}.{slot}] name {:?} -> {:?}",
                    a.display_name(),
                    b.display_name()
                ));
            }
            for (key, value) in &a.entries {
                let Some(other) = b.value(key) else {
                    problems.push(format!("[{index}.{slot}] lost {key}"));
                    continue;
                };
                if key == "text" {
                    if !same_text_buffer(value, other) {
                        problems.push(format!("[{index}.{slot}] text changed"));
                    }
                    continue;
                }
                if !same_track(value, other) {
                    problems.push(format!(
                        "[{index}.{slot}] {key}: {value:.40} -> {other:.40}"
                    ));
                }
            }
        }
    }
    problems
}

#[test]
#[ignore = "needs an AviUtl package; set AVIUTL_SAMPLES to its root"]
fn shipped_object_files_round_trip_without_changing_meaning() {
    let root = std::env::var("AVIUTL_SAMPLES")
        .map(PathBuf::from)
        .expect("set AVIUTL_SAMPLES to the AviUtl package root");
    let files = object_files(&root);
    assert!(!files.is_empty(), "no .exo files under {}", root.display());

    let (catalog, _) = EffectCatalog::load();
    let mut failures = Vec::new();
    for path in &files {
        let bytes = std::fs::read(path).expect("object file is readable");
        let original = ExoFile::parse(&bytes).expect("object file parses");
        let scene = scene_for(&original);
        let outcome = plan_import(&original, &scene, &catalog, 1, ImportPlacement::default());
        let document = ProjectDocument {
            version: 3,
            settings: ProjectSettings {
                width: original.header.width,
                height: original.header.height,
                fps: original.header.rate,
                sample_rate: original.header.audio_rate,
                extra: Default::default(),
            },
            scenes: vec![scene.clone()],
            clips: outcome.clips,
            extra: Default::default(),
        };
        let (exported, _) = export_scene(&document, &scene, &catalog);
        let reparsed = ExoFile::parse(&exported.to_bytes()).expect("exported file parses");
        for problem in compare(&original, &reparsed) {
            failures.push(format!("{}: {problem}", path.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} problem(s) across {} file(s):\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
