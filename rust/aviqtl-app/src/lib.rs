//! Framework-neutral application state shared by desktop frontends.

pub mod effect_catalog;
pub mod effect_selection;
pub mod media_import;
pub mod missing_media;
pub mod preset_store;
pub mod project_io;
pub mod selection;
pub mod settings;
pub mod timeline_interaction;
pub mod transport;
pub mod workspace;

pub use project_io::{ProjectDefaults, ProjectSession};
pub use workspace::WorkspaceModel;

use std::path::Path;

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
        Ok(self.add_project_session(project))
    }

    pub fn select_project(&mut self, index: usize) -> bool {
        if index >= self.projects.len() {
            return false;
        }
        self.current_project = Some(index);
        true
    }

    /// Closes a clean project. Dirty-project confirmation remains owned by the UI flow.
    pub fn close_clean_project(&mut self, index: usize) -> bool {
        if self
            .projects
            .get(index)
            .is_none_or(|project| project.workspace.project().dirty)
        {
            return false;
        }
        self.projects.remove(index);
        self.current_project = if self.projects.is_empty() {
            None
        } else {
            Some(index.min(self.projects.len() - 1))
        };
        true
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
}
