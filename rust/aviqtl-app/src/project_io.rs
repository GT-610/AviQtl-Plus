use aviqtl_rust_core::api::{ProjectDocument, TimelineState};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub struct ProjectSession {
    pub state: TimelineState,
    pub document: ProjectDocument,
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectDefaults {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub sample_rate: i32,
    pub duration: i32,
    pub enable_snap: bool,
    pub magnetic_snap_range: i32,
}

impl Default for ProjectDefaults {
    fn default() -> Self {
        Self {
            width: 1_920,
            height: 1_080,
            fps: 60.0,
            sample_rate: 48_000,
            duration: 3_600,
            enable_snap: true,
            magnetic_snap_range: 10,
        }
    }
}

impl ProjectSession {
    #[cfg(test)]
    pub fn blank() -> Self {
        Self::blank_with(ProjectDefaults::default())
    }

    pub fn blank_with(defaults: ProjectDefaults) -> Self {
        // Normalize caller input with the same bounds as the launcher settings
        // so the fixed document below always serializes and validates.
        let defaults = ProjectDefaults {
            width: defaults.width.clamp(1, 16_000),
            height: defaults.height.clamp(1, 16_000),
            fps: if defaults.fps.is_finite() {
                defaults.fps.clamp(1.0, 240.0)
            } else {
                60.0
            },
            sample_rate: defaults.sample_rate.clamp(8_000, 192_000),
            duration: defaults.duration.clamp(1, 1_000_000),
            enable_snap: defaults.enable_snap,
            magnetic_snap_range: defaults.magnetic_snap_range.clamp(1, 100),
        };
        let document = serde_json::json!({
            "version": 3,
            "settings": {
                "width": defaults.width,
                "height": defaults.height,
                "fps": defaults.fps,
                "sampleRate": defaults.sample_rate
            },
            "scenes": [{
                "id": 1,
                "name": "Root",
                "width": defaults.width,
                "height": defaults.height,
                "fps": defaults.fps,
                "duration": defaults.duration,
                "enableSnap": defaults.enable_snap,
                "magneticSnapRange": defaults.magnetic_snap_range
            }],
            "clips": []
        });
        let bytes = serde_json::to_vec(&document)
            .expect("the built-in blank project document must serialize");
        let state =
            TimelineState::from_json(&bytes).expect("the built-in blank project must remain valid");
        let document = state.snapshot();
        Self {
            state,
            document,
            path: None,
            dirty: false,
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut project =
            Self::from_json(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        project.path = Some(path.to_path_buf());
        Ok(project)
    }

    pub fn from_json(input: &[u8]) -> Result<Self, String> {
        let state = TimelineState::from_json(input).map_err(|error| error.to_string())?;
        let document = state.snapshot();
        Ok(Self {
            state,
            document,
            path: None,
            dirty: false,
        })
    }

    pub fn load_recovery(
        snapshot_path: &Path,
        _original_project_url: &str,
    ) -> Result<Self, String> {
        let bytes = fs::read(snapshot_path)
            .map_err(|error| format!("{}: {error}", snapshot_path.display()))?;
        let state = TimelineState::from_json(&bytes)
            .map_err(|error| format!("{}: {error}", snapshot_path.display()))?;
        let document = state.snapshot();
        Ok(Self {
            state,
            document,
            // Match the Qt recovery flow: the source path is informative only. A recovered
            // project is intentionally unsaved so that a normal Save opens Save As instead of
            // overwriting the original project.
            path: None,
            dirty: true,
        })
    }

    pub fn refresh(&mut self) {
        self.document = self.state.snapshot();
    }

    pub fn save(&mut self) -> Result<(), String> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| "choose a project path before saving".to_owned())?;
        let bytes = self.snapshot_bytes()?;
        write_atomic(path, &bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        self.dirty = false;
        Ok(())
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let bytes = self.snapshot_bytes()?;
        write_atomic(path, &bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        let json = self
            .state
            .snapshot_json()
            .map_err(|error| error.to_string())?;
        serde_json::to_vec_pretty(&json).map_err(|error| error.to_string())
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".aviqtl-save-")
        .tempfile_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    // Windows can reject simultaneous replace operations on the same target.
    // Serialize only publication; encoding and disk writes remain concurrent.
    static PUBLICATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _publication = PUBLICATION
        .lock()
        .map_err(|_| io::Error::other("save publication lock poisoned"))?;
    // Close the source handle before replacement, including on Windows where
    // a still-open replaced file can remain delete-pending during another save.
    temporary
        .into_temp_path()
        .persist(path)
        .map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::TimelineCommand;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn concurrent_saves_publish_complete_files_and_remove_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("project.aviqtl");
        write_atomic(&target, b"old project").unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for byte in 1..=8_u8 {
                let target = &target;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    write_atomic(target, &vec![byte; 64 * 1024]).unwrap();
                });
            }
        });
        let bytes = fs::read(&target).unwrap();
        assert_eq!(bytes.len(), 64 * 1024);
        assert!((1..=8).contains(&bytes[0]));
        assert!(bytes.iter().all(|byte| *byte == bytes[0]));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_save_as_keeps_the_old_project_and_dirty_state() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("project.aviqtl");
        let blocked = directory.path().join("directory.aviqtl");
        fs::create_dir(&blocked).unwrap();
        let mut project = ProjectSession::blank();
        project.save_as(&target).unwrap();
        let original = fs::read(&target).unwrap();
        project.dirty = true;
        assert!(project.save_as(&blocked).is_err());
        assert!(
            project
                .save_as(&directory.path().join("missing/project.aviqtl"))
                .is_err()
        );
        assert!(project.dirty);
        assert_eq!(project.path.as_deref(), Some(target.as_path()));
        assert_eq!(fs::read(&target).unwrap(), original);
        assert!(blocked.is_dir());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn blank_project_uses_configured_defaults() {
        let project = ProjectSession::blank_with(ProjectDefaults {
            width: 1_280,
            height: 720,
            fps: 30.0,
            sample_rate: 44_100,
            duration: 900,
            enable_snap: false,
            magnetic_snap_range: 18,
        });
        assert_eq!(project.document.settings.width, 1_280);
        assert_eq!(project.document.settings.height, 720);
        assert_eq!(project.document.settings.fps, 30.0);
        assert_eq!(project.document.settings.sample_rate, 44_100);
        assert_eq!(project.document.scenes[0].duration, 900);
        assert_eq!(project.document.scenes[0].fps, 30.0);
        assert!(!project.document.scenes[0].enable_snap);
        assert_eq!(project.document.scenes[0].magnetic_snap_range, 18);
    }

    #[test]
    fn save_and_reopen_preserves_a_typed_edit() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aviqtl-egui-project-{}-{nonce}.aviqtl",
            std::process::id()
        ));
        let mut project = ProjectSession::blank();
        let mut scene = project.document.scenes[0].clone();
        scene.name = "Edited in Rust".to_owned();
        let transaction = project
            .state
            .plan(TimelineCommand::UpdateScene {
                scene_id: scene.id,
                scene,
            })
            .expect("scene edit plans");
        project
            .state
            .apply(&transaction)
            .expect("scene edit applies");
        project.refresh();
        project.path = Some(path.clone());
        project.dirty = true;
        project.save().expect("project saves");

        let reopened = ProjectSession::load(&path).expect("saved project reopens");
        assert_eq!(reopened.document.scenes[0].name, "Edited in Rust");
        assert!(!reopened.dirty);
        fs::remove_file(path).expect("test project removes");
    }

    #[test]
    fn failed_save_as_preserves_the_previous_path_and_dirty_state() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aviqtl-save-as-directory-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory creates");
        let mut project = ProjectSession::blank();
        project.dirty = true;

        assert!(project.save_as(&path).is_err());
        assert_eq!(project.path, None);
        assert!(project.dirty);

        fs::remove_dir(path).expect("fixture directory removes");
    }

    #[test]
    fn recovery_stays_unsaved_and_dirty_even_when_the_original_path_is_known() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        let snapshot_path = std::env::temp_dir().join(format!(
            "aviqtl-egui-recovery-{}-{nonce}.aviqtl",
            std::process::id()
        ));
        let project = ProjectSession::blank();
        fs::write(
            &snapshot_path,
            project.snapshot_bytes().expect("snapshot serializes"),
        )
        .expect("recovery snapshot writes");

        let recovered =
            ProjectSession::load_recovery(&snapshot_path, "file:///tmp/AviQtl%20recovery.aviqtl")
                .expect("recovery snapshot loads");
        assert_eq!(recovered.path, None);
        assert!(recovered.dirty);

        let unsaved = ProjectSession::load_recovery(&snapshot_path, "")
            .expect("unsaved recovery snapshot loads");
        assert_eq!(unsaved.path, None);
        assert!(unsaved.dirty);
        fs::remove_file(snapshot_path).expect("recovery snapshot removes");
    }
}
