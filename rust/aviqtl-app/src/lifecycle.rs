use std::path::PathBuf;

/// Answer chosen in the unsaved-project confirmation shown before close or quit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveDecision {
    Save,
    Discard,
    Cancel,
}

/// The next presentation action required by the framework-neutral lifecycle flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleStep {
    None,
    ConfirmSave {
        project_index: usize,
        project_name: String,
    },
    ChooseSavePath {
        project_index: usize,
        suggested_path: PathBuf,
    },
    ProjectSaved {
        project_index: usize,
    },
    ProjectClosed {
        project_index: usize,
    },
    QuitReady,
    Cancelled,
    SaveFailed {
        project_index: usize,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleContinuation {
    CloseProject(usize),
    ContinueQuit(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingLifecycle {
    ConfirmSave {
        project_index: usize,
        continuation: LifecycleContinuation,
    },
    ChooseSavePath {
        project_index: usize,
        continuation: Option<LifecycleContinuation>,
    },
}
