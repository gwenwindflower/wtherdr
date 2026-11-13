use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceWorkspace {
    pub workspace_id: String,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCheckout {
    pub workspace_id: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkflowOutcome {
    pub message: String,
    pub completion_source: SourceWorkspace,
    pub close_workspace_ids: Vec<String>,
    pub cancelled: bool,
    pub error: Option<String>,
}

impl WorkflowOutcome {
    pub fn completed(repository: &RepositoryContext, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            completion_source: SourceWorkspace {
                workspace_id: repository.parent_workspace_id.clone(),
                cwd: repository.root_path.clone(),
            },
            close_workspace_ids: Vec::new(),
            cancelled: false,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryContext {
    pub parent_workspace_id: String,
    pub root_path: PathBuf,
    pub repo_name: String,
    pub source_label: Option<String>,
    pub source_is_linked_worktree: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkout {
    pub branch: Option<String>,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeOutcome {
    pub removed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwitchMode {
    Pick,
    Create {
        branch: String,
        base: Option<String>,
        focus: bool,
    },
}
