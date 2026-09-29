use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceWorkspace {
    pub workspace_id: String,
    pub cwd: PathBuf,
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
    },
}
