//! Framework-neutral application state shared by desktop frontends.

pub mod effect_catalog;
pub mod effect_selection;
pub mod lifecycle;
pub mod media_import;
pub mod missing_media;
pub mod preset_store;
pub mod project_io;
pub mod selection;
pub mod settings;
pub mod timeline_interaction;
pub mod transport;
pub mod workspace;

pub use lifecycle::{LifecycleStep, SaveDecision};
pub use project_io::{ProjectDefaults, ProjectSession};
pub use workspace::WorkspaceModel;

use lifecycle::{LifecycleContinuation, PendingLifecycle};
use std::path::{Path, PathBuf};

/// Snapshot used by a UI tab strip without exposing the mutable project model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTab {
    pub name: String,
    pub dirty: bool,
}

/// Project/window lifecycle state that does not depend on a GUI framework.
pub struct ApplicationModel {
    projects: Vec<ProjectEntry>,
    current_project: Option<usize>,
    next_untitled_number: u32,
    pending_lifecycle: Option<PendingLifecycle>,
}

struct ProjectEntry {
    workspace: WorkspaceModel,
    untitled_name: String,
}

impl Default for ApplicationModel {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            current_project: None,
            next_untitled_number: 1,
            pending_lifecycle: None,
        }
    }
}

impl ApplicationModel {
    pub fn launcher_visible(&self) -> bool {
        self.projects.is_empty()
    }

    pub fn project_count(&self) -> usize {
        self.projects.len()
    }

    pub fn current_project_index(&self) -> Option<usize> {
        self.current_project
    }

    pub fn lifecycle_pending(&self) -> bool {
        self.pending_lifecycle.is_some()
    }

    pub fn create_project(&mut self, defaults: ProjectDefaults) -> usize {
        self.add_project_session(ProjectSession::blank_with(defaults))
    }

    pub fn add_project_session(&mut self, project: ProjectSession) -> usize {
        let index = self.projects.len();
        let untitled_name = format!("Untitled {}", self.next_untitled_number);
        self.projects.push(ProjectEntry {
            workspace: WorkspaceModel::new(project),
            untitled_name,
        });
        self.current_project = Some(index);
        self.next_untitled_number += 1;
        index
    }

    pub fn open_project(&mut self, path: &Path) -> Result<usize, String> {
        let project = ProjectSession::load(path)?;
        if let Some(index) = self.current_project
            && self.projects[index].workspace.project().path.is_none()
            && !self.projects[index].workspace.project().dirty
        {
            self.projects[index].workspace = WorkspaceModel::new(project);
            return Ok(index);
        }
        Ok(self.add_project_session(project))
    }

    pub fn select_project(&mut self, index: usize) -> bool {
        if index >= self.projects.len() {
            return false;
        }
        self.current_project = Some(index);
        true
    }

    /// Closes a clean project immediately. User-facing close requests should use the lifecycle flow.
    pub fn close_clean_project(&mut self, index: usize) -> bool {
        if self
            .projects
            .get(index)
            .is_none_or(|project| project.workspace.project().dirty)
        {
            return false;
        }
        self.remove_project(index);
        true
    }

    pub fn request_save_current_project(&mut self) -> LifecycleStep {
        if self.pending_lifecycle.is_some() {
            return LifecycleStep::None;
        }
        let Some(index) = self.current_project else {
            return LifecycleStep::None;
        };
        if self.projects[index].workspace.project().path.is_none() {
            return self.begin_save_as(index, None);
        }
        self.save_project(index, None)
    }

    pub fn request_save_current_project_as(&mut self) -> LifecycleStep {
        if self.pending_lifecycle.is_some() {
            return LifecycleStep::None;
        }
        let Some(index) = self.current_project else {
            return LifecycleStep::None;
        };
        self.begin_save_as(index, None)
    }

    pub fn request_close_project(&mut self, index: usize) -> LifecycleStep {
        if self.pending_lifecycle.is_some() || index >= self.projects.len() {
            return LifecycleStep::None;
        }
        self.current_project = Some(index);
        if self.projects[index].workspace.project().dirty {
            self.begin_confirmation(index, LifecycleContinuation::CloseProject(index))
        } else {
            self.remove_project(index);
            LifecycleStep::ProjectClosed {
                project_index: index,
            }
        }
    }

    pub fn request_quit(&mut self, confirm_unsaved: bool) -> LifecycleStep {
        if self.pending_lifecycle.is_some() {
            return LifecycleStep::None;
        }
        if confirm_unsaved {
            self.continue_quit(0)
        } else {
            LifecycleStep::QuitReady
        }
    }

    pub fn answer_save_confirmation(&mut self, decision: SaveDecision) -> LifecycleStep {
        let Some(PendingLifecycle::ConfirmSave {
            project_index,
            continuation,
        }) = self.pending_lifecycle.take()
        else {
            return LifecycleStep::None;
        };

        match decision {
            SaveDecision::Save => {
                if self.projects[project_index]
                    .workspace
                    .project()
                    .path
                    .is_none()
                {
                    self.begin_save_as(project_index, Some(continuation))
                } else {
                    self.save_project(project_index, Some(continuation))
                }
            }
            SaveDecision::Discard => self.complete_continuation(continuation),
            SaveDecision::Cancel => LifecycleStep::Cancelled,
        }
    }

    pub fn complete_save_path(&mut self, path: Option<&Path>) -> LifecycleStep {
        let Some(PendingLifecycle::ChooseSavePath {
            project_index,
            continuation,
        }) = self.pending_lifecycle.take()
        else {
            return LifecycleStep::None;
        };
        let Some(path) = path else {
            return LifecycleStep::Cancelled;
        };

        match self.projects[project_index]
            .workspace
            .project_mut()
            .save_as(path)
        {
            Ok(()) => continuation.map_or(
                LifecycleStep::ProjectSaved { project_index },
                |continuation| self.complete_continuation(continuation),
            ),
            Err(message) => LifecycleStep::SaveFailed {
                project_index,
                message,
            },
        }
    }

    pub fn tabs(&self) -> Vec<ProjectTab> {
        self.projects
            .iter()
            .map(|project| ProjectTab {
                name: project
                    .workspace
                    .project()
                    .path
                    .as_deref()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| project.untitled_name.clone()),
                dirty: project.workspace.project().dirty,
            })
            .collect()
    }

    pub fn workspace(&self, index: usize) -> Option<&WorkspaceModel> {
        self.projects.get(index).map(|project| &project.workspace)
    }

    pub fn workspace_mut(&mut self, index: usize) -> Option<&mut WorkspaceModel> {
        self.projects
            .get_mut(index)
            .map(|project| &mut project.workspace)
    }

    pub fn current_workspace(&self) -> Option<&WorkspaceModel> {
        self.current_project.and_then(|index| self.workspace(index))
    }

    pub fn current_workspace_mut(&mut self) -> Option<&mut WorkspaceModel> {
        let index = self.current_project?;
        self.workspace_mut(index)
    }

    fn begin_confirmation(
        &mut self,
        project_index: usize,
        continuation: LifecycleContinuation,
    ) -> LifecycleStep {
        self.pending_lifecycle = Some(PendingLifecycle::ConfirmSave {
            project_index,
            continuation,
        });
        LifecycleStep::ConfirmSave {
            project_index,
            project_name: self.project_name(project_index),
        }
    }

    fn begin_save_as(
        &mut self,
        project_index: usize,
        continuation: Option<LifecycleContinuation>,
    ) -> LifecycleStep {
        self.pending_lifecycle = Some(PendingLifecycle::ChooseSavePath {
            project_index,
            continuation,
        });
        LifecycleStep::ChooseSavePath {
            project_index,
            suggested_path: self.suggested_save_path(project_index),
        }
    }

    fn save_project(
        &mut self,
        project_index: usize,
        continuation: Option<LifecycleContinuation>,
    ) -> LifecycleStep {
        match self.projects[project_index].workspace.project_mut().save() {
            Ok(()) => continuation.map_or(
                LifecycleStep::ProjectSaved { project_index },
                |continuation| self.complete_continuation(continuation),
            ),
            Err(message) => LifecycleStep::SaveFailed {
                project_index,
                message,
            },
        }
    }

    fn complete_continuation(&mut self, continuation: LifecycleContinuation) -> LifecycleStep {
        match continuation {
            LifecycleContinuation::CloseProject(project_index) => {
                self.remove_project(project_index);
                LifecycleStep::ProjectClosed { project_index }
            }
            LifecycleContinuation::ContinueQuit(next_index) => self.continue_quit(next_index),
        }
    }

    fn continue_quit(&mut self, start_index: usize) -> LifecycleStep {
        let next_dirty = self
            .projects
            .iter()
            .enumerate()
            .skip(start_index)
            .find_map(|(index, project)| project.workspace.project().dirty.then_some(index));
        let Some(project_index) = next_dirty else {
            return LifecycleStep::QuitReady;
        };
        self.current_project = Some(project_index);
        self.begin_confirmation(
            project_index,
            LifecycleContinuation::ContinueQuit(project_index + 1),
        )
    }

    fn remove_project(&mut self, index: usize) {
        self.projects.remove(index);
        self.current_project = if self.projects.is_empty() {
            None
        } else {
            Some(index.min(self.projects.len() - 1))
        };
    }

    fn project_name(&self, index: usize) -> String {
        let project = &self.projects[index];
        project
            .workspace
            .project()
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| project.untitled_name.clone())
    }

    fn suggested_save_path(&self, index: usize) -> PathBuf {
        let project = &self.projects[index];
        project
            .workspace
            .project()
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}.aviqtl", project.untitled_name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_is_visible_until_a_project_exists() {
        let mut app = ApplicationModel::default();
        assert!(app.launcher_visible());
        app.create_project(ProjectDefaults::default());
        assert!(!app.launcher_visible());
    }

    #[test]
    fn project_tabs_follow_qt_selection_and_close_order() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        app.create_project(ProjectDefaults::default());
        app.create_project(ProjectDefaults::default());
        assert_eq!(app.current_project_index(), Some(2));
        assert!(app.select_project(1));
        assert!(app.close_clean_project(1));
        assert_eq!(app.project_count(), 2);
        assert_eq!(app.current_project_index(), Some(1));
        assert_eq!(app.tabs()[1].name, "Untitled 3");
    }

    #[test]
    fn invalid_project_selection_preserves_current_tab() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        assert!(!app.select_project(10));
        assert_eq!(app.current_project_index(), Some(0));
    }

    #[test]
    fn opening_replaces_only_the_clean_pathless_qt_placeholder() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        let path = temporary_project_path("replace-placeholder");
        ProjectSession::blank_with(ProjectDefaults::default())
            .save_as(&path)
            .expect("fixture project saves");

        assert_eq!(app.open_project(&path), Ok(0));
        assert_eq!(app.project_count(), 1);
        assert_eq!(
            app.current_workspace()
                .and_then(|workspace| workspace.project().path.as_deref()),
            Some(path.as_path())
        );

        std::fs::remove_file(path).expect("fixture project removes");
    }

    #[test]
    fn opening_keeps_a_dirty_pathless_project_and_adds_a_tab() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        app.current_workspace_mut()
            .expect("project exists")
            .project_mut()
            .dirty = true;
        let path = temporary_project_path("keep-dirty-placeholder");
        ProjectSession::blank_with(ProjectDefaults::default())
            .save_as(&path)
            .expect("fixture project saves");

        assert_eq!(app.open_project(&path), Ok(1));
        assert_eq!(app.project_count(), 2);
        assert_eq!(app.current_project_index(), Some(1));

        std::fs::remove_file(path).expect("fixture project removes");
    }

    #[test]
    fn direct_save_falls_back_to_save_as_and_then_saves_in_place() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        app.current_workspace_mut()
            .expect("project exists")
            .project_mut()
            .dirty = true;
        let path = temporary_project_path("direct-save");

        assert_eq!(
            app.request_save_current_project(),
            LifecycleStep::ChooseSavePath {
                project_index: 0,
                suggested_path: PathBuf::from("Untitled 1.aviqtl"),
            }
        );
        assert_eq!(
            app.complete_save_path(Some(&path)),
            LifecycleStep::ProjectSaved { project_index: 0 }
        );
        app.current_workspace_mut()
            .expect("project exists")
            .project_mut()
            .dirty = true;
        assert_eq!(
            app.request_save_current_project(),
            LifecycleStep::ProjectSaved { project_index: 0 }
        );
        assert!(
            !app.current_workspace()
                .expect("project exists")
                .project()
                .dirty
        );

        std::fs::remove_file(path).expect("saved project removes");
    }

    #[test]
    fn dirty_project_close_supports_cancel_discard_and_pathless_save_as() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        app.current_workspace_mut()
            .expect("project exists")
            .project_mut()
            .dirty = true;

        assert!(matches!(
            app.request_close_project(0),
            LifecycleStep::ConfirmSave {
                project_index: 0,
                ..
            }
        ));
        assert_eq!(
            app.answer_save_confirmation(SaveDecision::Cancel),
            LifecycleStep::Cancelled
        );
        assert_eq!(app.project_count(), 1);

        assert!(matches!(
            app.request_close_project(0),
            LifecycleStep::ConfirmSave { .. }
        ));
        assert!(matches!(
            app.answer_save_confirmation(SaveDecision::Save),
            LifecycleStep::ChooseSavePath {
                project_index: 0,
                ..
            }
        ));
        assert_eq!(app.complete_save_path(None), LifecycleStep::Cancelled);
        assert_eq!(app.project_count(), 1);

        assert!(matches!(
            app.request_close_project(0),
            LifecycleStep::ConfirmSave { .. }
        ));
        assert_eq!(
            app.answer_save_confirmation(SaveDecision::Discard),
            LifecycleStep::ProjectClosed { project_index: 0 }
        );
        assert!(app.launcher_visible());
    }

    #[test]
    fn quit_visits_dirty_projects_in_tab_order() {
        let mut app = ApplicationModel::default();
        for dirty in [false, true, false, true] {
            let index = app.create_project(ProjectDefaults::default());
            app.workspace_mut(index)
                .expect("project exists")
                .project_mut()
                .dirty = dirty;
        }

        assert!(matches!(
            app.request_quit(true),
            LifecycleStep::ConfirmSave {
                project_index: 1,
                ..
            }
        ));
        assert!(matches!(
            app.answer_save_confirmation(SaveDecision::Discard),
            LifecycleStep::ConfirmSave {
                project_index: 3,
                ..
            }
        ));
        assert_eq!(
            app.answer_save_confirmation(SaveDecision::Discard),
            LifecycleStep::QuitReady
        );
        assert_eq!(app.project_count(), 4);
    }

    #[test]
    fn disabled_quit_confirmation_skips_dirty_projects() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        app.current_workspace_mut()
            .expect("project exists")
            .project_mut()
            .dirty = true;

        assert_eq!(app.request_quit(false), LifecycleStep::QuitReady);
        assert_eq!(app.project_count(), 1);
    }

    #[test]
    fn successful_save_as_resumes_a_deferred_close() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        app.current_workspace_mut()
            .expect("project exists")
            .project_mut()
            .dirty = true;
        let path = temporary_project_path("deferred-close");

        assert!(matches!(
            app.request_close_project(0),
            LifecycleStep::ConfirmSave { .. }
        ));
        assert!(matches!(
            app.answer_save_confirmation(SaveDecision::Save),
            LifecycleStep::ChooseSavePath { .. }
        ));
        assert_eq!(
            app.complete_save_path(Some(&path)),
            LifecycleStep::ProjectClosed { project_index: 0 }
        );
        assert!(path.exists());
        assert!(app.launcher_visible());

        std::fs::remove_file(path).expect("saved project removes");
    }

    #[test]
    fn failed_save_cancels_the_deferred_close() {
        let mut app = ApplicationModel::default();
        app.create_project(ProjectDefaults::default());
        app.current_workspace_mut()
            .expect("project exists")
            .project_mut()
            .dirty = true;
        let path = temporary_project_path("missing-parent").join("project.aviqtl");

        app.request_close_project(0);
        app.answer_save_confirmation(SaveDecision::Save);
        assert!(matches!(
            app.complete_save_path(Some(&path)),
            LifecycleStep::SaveFailed {
                project_index: 0,
                ..
            }
        ));
        assert_eq!(app.project_count(), 1);
        assert!(
            app.current_workspace()
                .expect("project remains")
                .project()
                .dirty
        );
        assert_eq!(
            app.current_workspace()
                .expect("project remains")
                .project()
                .path,
            None
        );
        assert_eq!(
            app.answer_save_confirmation(SaveDecision::Discard),
            LifecycleStep::None
        );
    }

    fn temporary_project_path(label: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "aviqtl-slint-{label}-{}-{nonce}.aviqtl",
            std::process::id()
        ))
    }
}
