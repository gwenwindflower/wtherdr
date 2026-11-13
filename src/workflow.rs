use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::model::{
    Checkout, MergeOutcome, RepositoryContext, SourceWorkspace, SwitchMode, WorkflowOutcome,
    WorkspaceCheckout,
};

pub trait Herdr {
    fn repository_context(&self, source: &SourceWorkspace) -> Result<RepositoryContext>;
    fn open_worktree(
        &self,
        parent_workspace_id: &str,
        checkout: &Checkout,
        focus: bool,
    ) -> Result<()>;
    fn focus_workspace(&self, workspace_id: &str) -> Result<()>;
    fn close_workspace(&self, workspace_id: &str) -> Result<()>;
    fn linked_workspaces(&self, source: &SourceWorkspace) -> Result<Vec<WorkspaceCheckout>>;
}

pub trait Worktrunk {
    fn clean_candidates(&self, cwd: &Path) -> Result<Vec<std::path::PathBuf>>;
    fn switch(&self, cwd: &Path, mode: &SwitchMode) -> Result<Option<Checkout>>;
    fn merge(&self, cwd: &Path) -> Result<MergeOutcome>;
    fn remove(&self, cwd: &Path) -> Result<()>;
    fn worktree_paths(&self, cwd: &Path) -> Result<Vec<std::path::PathBuf>>;
}

pub fn require_linked_worktree(repository: &RepositoryContext, operation: &str) -> Result<()> {
    if repository.source_is_linked_worktree {
        Ok(())
    } else {
        bail!(
            "{operation} must run from a nested worktree workspace; the repository parent is not removable"
        )
    }
}

pub struct Engine<H, W> {
    herdr: H,
    worktrunk: W,
}

impl<H, W> Engine<H, W>
where
    H: Herdr,
    W: Worktrunk,
{
    pub fn new(herdr: H, worktrunk: W) -> Self {
        Self { herdr, worktrunk }
    }

    pub fn switch(&self, source: &SourceWorkspace, mode: &SwitchMode) -> Result<WorkflowOutcome> {
        let repository = self.herdr.repository_context(source)?;
        let registered = if matches!(mode, SwitchMode::Pick) {
            self.herdr.linked_workspaces(source)?
        } else {
            Vec::new()
        };
        let before = if matches!(mode, SwitchMode::Pick) {
            self.worktrunk.worktree_paths(&repository.root_path)?
        } else {
            Vec::new()
        };
        let tracked: Vec<WorkspaceCheckout> = registered
            .into_iter()
            .filter_map(|workspace| {
                before
                    .iter()
                    .find(|path| same_path(path, &workspace.path))
                    .map(|path| WorkspaceCheckout {
                        workspace_id: workspace.workspace_id,
                        path: path.clone(),
                    })
            })
            .collect();
        let selected = self
            .worktrunk
            .switch(&source.cwd, mode)
            .context("Worktrunk could not switch worktrees");
        let mut outcome = WorkflowOutcome::completed(&repository, "Worktree switch completed");
        let mut removed = Vec::new();
        if matches!(mode, SwitchMode::Pick) {
            match self.worktrunk.worktree_paths(&repository.root_path)
                .context("Worktrunk's picker exited, but checkout cleanup could not be verified; Herdr workspaces remain open") {
                Ok(after) => {
                    removed = before.into_iter().filter(|path| !after.iter().any(|current| same_path(path, current))).collect();
                    outcome.close_workspace_ids = tracked.into_iter()
                        .filter(|workspace| removed.iter().any(|path| same_path(path, &workspace.path)))
                        .map(|workspace| workspace.workspace_id).collect();
                }
                Err(error) => append_failure(&mut outcome, &error),
            }
        }
        let removed_message = format!(
            "Removed worktrees in the switch picker: {}",
            removed
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let checkout = match selected {
            Ok(Some(checkout)) => checkout,
            Ok(None) => {
                outcome.cancelled = removed.is_empty() && outcome.error.is_none();
                outcome.message = if removed.is_empty() {
                    "Switch picker closed".into()
                } else {
                    removed_message
                };
                return Ok(outcome);
            }
            Err(error) if removed.is_empty() && outcome.error.is_none() => {
                return Err(error);
            }
            Err(error) => {
                append_failure(&mut outcome, &error);
                outcome.message = if removed.is_empty() {
                    "Switch picker failed".into()
                } else {
                    removed_message
                };
                return Ok(outcome);
            }
        };
        outcome.message = format!(
            "{} {}",
            if matches!(mode, SwitchMode::Create { .. }) {
                "Created"
            } else {
                "Switched to"
            },
            checkout.branch.as_deref().unwrap_or("detached checkout")
        );
        if !removed.is_empty() {
            outcome.message.push('\n');
            outcome.message.push_str(&removed_message);
        }
        if let Err(error) = self.open_checkout(&repository, &checkout, mode) {
            if removed.is_empty() && outcome.error.is_none() {
                return Err(error);
            }
            append_failure(&mut outcome, &error);
        } else if matches!(mode, SwitchMode::Pick)
            && !same_path(&checkout.path, &repository.root_path)
        {
            match self.checkout_workspace(source, &checkout) {
                Ok(workspace) => outcome.completion_source = workspace,
                Err(error) => append_failure(&mut outcome, &error),
            }
        }
        Ok(outcome)
    }

    fn checkout_workspace(
        &self,
        source: &SourceWorkspace,
        checkout: &Checkout,
    ) -> Result<SourceWorkspace> {
        self.herdr
            .linked_workspaces(source)?
            .into_iter()
            .find(|workspace| same_path(&workspace.path, &checkout.path))
            .map(|workspace| SourceWorkspace {
                workspace_id: workspace.workspace_id,
                cwd: checkout.path.clone(),
            })
            .with_context(|| {
                format!(
                    "Herdr opened {}, but its workspace could not be found for the result popup",
                    checkout.path.display()
                )
            })
    }

    fn open_checkout(
        &self,
        repository: &RepositoryContext,
        checkout: &Checkout,
        mode: &SwitchMode,
    ) -> Result<()> {
        if same_path(&checkout.path, &repository.root_path) {
            self.herdr
                .focus_workspace(&repository.parent_workspace_id)
                .context("Worktrunk switched successfully, but Herdr could not focus the parent workspace")
        } else {
            let focus = match mode {
                SwitchMode::Pick => true,
                SwitchMode::Create { focus, .. } => *focus,
            };
            self.herdr
                .open_worktree(&repository.parent_workspace_id, checkout, focus)
                .with_context(|| {
                    format!(
                        "Worktrunk switched to {}, but Herdr could not open it as a nested workspace",
                        checkout.path.display()
                    )
                })
        }
    }

    pub fn clean(&self, source: &SourceWorkspace) -> Result<WorkflowOutcome> {
        let repository = self.herdr.repository_context(source)?;
        let candidates = self.worktrunk.clean_candidates(&repository.root_path)?;
        let before = self.worktrunk.worktree_paths(&repository.root_path)?;
        let candidates: Vec<_> = candidates
            .into_iter()
            .filter(|path| {
                !same_path(path, &repository.root_path)
                    && before.iter().any(|existing| same_path(path, existing))
            })
            .collect();
        let tracked: Vec<_> = self
            .herdr
            .linked_workspaces(source)?
            .into_iter()
            .filter_map(|workspace| {
                candidates
                    .iter()
                    .find(|path| same_path(path, &workspace.path))
                    .map(|path| WorkspaceCheckout {
                        workspace_id: workspace.workspace_id,
                        path: path.clone(),
                    })
            })
            .collect();
        let mut outcome =
            WorkflowOutcome::completed(&repository, "No integrated worktrees to clean");
        for path in &candidates {
            if let Err(error) = self
                .worktrunk
                .remove(path)
                .with_context(|| format!("Could not clean worktree {}", path.display()))
            {
                append_failure(&mut outcome, &error);
            }
        }
        match self.worktrunk.worktree_paths(&repository.root_path)
            .context("Cleanup ran, but checkout removal could not be verified; Herdr workspaces remain open") {
            Ok(after) => {
                let removed: Vec<_> = candidates.iter().filter(|path| !after.iter().any(|current| same_path(path, current))).collect();
                if !candidates.is_empty() {
                    outcome.message = format!("Cleaned {} of {} integrated worktrees", removed.len(), candidates.len());
                }
                outcome.close_workspace_ids = tracked.into_iter()
                    .filter(|workspace| removed.iter().any(|path| same_path(path, &workspace.path)))
                    .map(|workspace| workspace.workspace_id).collect();
            }
            Err(error) => append_failure(&mut outcome, &error),
        }
        if let Err(error) = self.focus_parent(&repository, "cleaned integrated worktrees from") {
            append_failure(&mut outcome, &error);
            outcome.close_workspace_ids.clear();
        }
        Ok(outcome)
    }

    pub fn merge(&self, source: &SourceWorkspace) -> Result<WorkflowOutcome> {
        let repository = self.linked_repository(source, "merge")?;
        let outcome = self
            .worktrunk
            .merge(&source.cwd)
            .context("Worktrunk could not merge this worktree")?;
        if outcome.removed {
            Ok(self.finish_removal(source, &repository, "merged"))
        } else {
            self.focus_parent(&repository, "merged")?;
            Ok(WorkflowOutcome::completed(
                &repository,
                "Merged the checkout",
            ))
        }
    }

    pub fn remove(&self, source: &SourceWorkspace) -> Result<WorkflowOutcome> {
        let repository = self.linked_repository(source, "remove")?;
        self.worktrunk
            .remove(&source.cwd)
            .context("Worktrunk could not remove this worktree")?;
        Ok(self.finish_removal(source, &repository, "removed"))
    }

    fn linked_repository(
        &self,
        source: &SourceWorkspace,
        operation: &str,
    ) -> Result<RepositoryContext> {
        let repository = self.herdr.repository_context(source)?;
        require_linked_worktree(&repository, operation)?;
        Ok(repository)
    }

    fn finish_removal(
        &self,
        source: &SourceWorkspace,
        repository: &RepositoryContext,
        operation: &str,
    ) -> WorkflowOutcome {
        let mut outcome =
            WorkflowOutcome::completed(repository, format!("Worktrunk {operation} the checkout"));
        if let Err(error) = self.focus_parent(repository, operation) {
            outcome.error = Some(format!(
                "{error:#}; workspace {} remains open",
                source.workspace_id
            ));
            return outcome;
        }
        outcome
            .close_workspace_ids
            .push(source.workspace_id.clone());
        outcome
    }

    fn focus_parent(&self, repository: &RepositoryContext, operation: &str) -> Result<()> {
        self.herdr
            .focus_workspace(&repository.parent_workspace_id)
            .with_context(|| {
                format!(
                    "Worktrunk {operation} the checkout, but Herdr could not focus parent workspace {}",
                    repository.parent_workspace_id
                )
            })
    }
}

fn append_failure(outcome: &mut WorkflowOutcome, error: &anyhow::Error) {
    let details = format!("{error:#}");
    outcome.error = Some(match outcome.error.take() {
        Some(previous) => format!("{previous}\n{details}"),
        None => details,
    });
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use anyhow::{Result, anyhow};

    use super::*;

    #[derive(Default)]
    struct FakeHerdr {
        context: Option<RepositoryContext>,
        calls: RefCell<Vec<String>>,
        registered: Vec<WorkspaceCheckout>,
        fail_open: bool,
        fail_focus: bool,
    }

    impl Herdr for FakeHerdr {
        fn linked_workspaces(&self, _: &SourceWorkspace) -> Result<Vec<WorkspaceCheckout>> {
            let mut registered = self.registered.clone();
            if self
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("open:w1:feature:/repo.feature:"))
            {
                registered.push(WorkspaceCheckout {
                    workspace_id: "w2".into(),
                    path: PathBuf::from("/repo.feature"),
                });
            }
            Ok(registered)
        }

        fn repository_context(&self, _: &SourceWorkspace) -> Result<RepositoryContext> {
            self.context
                .clone()
                .ok_or_else(|| anyhow!("missing context"))
        }

        fn open_worktree(&self, parent: &str, checkout: &Checkout, focus: bool) -> Result<()> {
            if self.fail_open {
                bail!("Herdr unavailable");
            }
            self.calls.borrow_mut().push(format!(
                "open:{parent}:{}:{}:{focus}",
                checkout.branch.as_deref().unwrap_or("detached"),
                checkout.path.display()
            ));
            Ok(())
        }

        fn focus_workspace(&self, workspace_id: &str) -> Result<()> {
            if self.fail_focus {
                bail!("focus unavailable");
            }
            self.calls
                .borrow_mut()
                .push(format!("focus:{workspace_id}"));
            Ok(())
        }

        fn close_workspace(&self, workspace_id: &str) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("close:{workspace_id}"));
            Ok(())
        }
    }

    struct FakeWorktrunk {
        checkout: Option<Checkout>,
        merge_removed: bool,
        calls: RefCell<Vec<String>>,
        fail: bool,
        paths: RefCell<std::collections::VecDeque<Vec<PathBuf>>>,
        fail_snapshot: bool,
        remove_on_switch: Option<PathBuf>,
    }

    impl Worktrunk for FakeWorktrunk {
        fn clean_candidates(&self, _: &Path) -> Result<Vec<PathBuf>> {
            Ok(vec![
                PathBuf::from("/repo.feature"),
                PathBuf::from("/repo.unopened"),
            ])
        }
        fn worktree_paths(&self, _: &Path) -> Result<Vec<PathBuf>> {
            match self.paths.borrow_mut().pop_front() {
                Some(paths) => Ok(paths),
                None if self.fail_snapshot => bail!("snapshot failed"),
                None => Ok(vec![PathBuf::from("/repo"), PathBuf::from("/repo.feature")]),
            }
        }

        fn switch(&self, cwd: &Path, mode: &SwitchMode) -> Result<Option<Checkout>> {
            if let Some(path) = &self.remove_on_switch {
                std::fs::remove_dir(path)?;
            }
            self.calls
                .borrow_mut()
                .push(format!("switch:{}:{mode:?}", cwd.display()));
            if self.fail {
                return Err(anyhow!("switch failed"));
            }
            Ok(self.checkout.clone())
        }

        fn merge(&self, cwd: &Path) -> Result<MergeOutcome> {
            self.calls
                .borrow_mut()
                .push(format!("merge:{}", cwd.display()));
            if self.fail {
                return Err(anyhow!("merge failed"));
            }
            Ok(MergeOutcome {
                removed: self.merge_removed,
            })
        }

        fn remove(&self, cwd: &Path) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("remove:{}", cwd.display()));
            if self.fail {
                return Err(anyhow!("remove failed"));
            }
            Ok(())
        }
    }

    fn source() -> SourceWorkspace {
        SourceWorkspace {
            workspace_id: "w2".into(),
            cwd: PathBuf::from("/repo.feature"),
        }
    }

    fn context(linked: bool) -> RepositoryContext {
        RepositoryContext {
            parent_workspace_id: "w1".into(),
            root_path: PathBuf::from("/repo"),
            repo_name: "repo".into(),
            source_label: Some("feature".into()),
            source_is_linked_worktree: linked,
        }
    }

    fn engine(checkout: Checkout, linked: bool, fail: bool) -> Engine<FakeHerdr, FakeWorktrunk> {
        Engine::new(
            FakeHerdr {
                context: Some(context(linked)),
                ..Default::default()
            },
            FakeWorktrunk {
                checkout: Some(checkout),
                merge_removed: true,
                calls: RefCell::default(),
                fail,
                paths: RefCell::default(),
                fail_snapshot: false,
                remove_on_switch: None,
            },
        )
    }

    #[test]
    fn clean_without_eligible_checkouts_preserves_workspaces_and_focuses_main() {
        let engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            false,
            false,
        );
        *engine.worktrunk.paths.borrow_mut() =
            [vec![PathBuf::from("/repo")], vec![PathBuf::from("/repo")]].into();

        let outcome = engine.clean(&source()).unwrap();

        assert!(engine.worktrunk.calls.borrow().is_empty());
        assert_eq!(outcome.close_workspace_ids, Vec::<String>::new());
        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
        assert_eq!(outcome.message, "No integrated worktrees to clean");
    }

    #[test]
    fn clean_keeps_workspaces_open_when_checkout_removal_cannot_be_verified() {
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            false,
        );
        engine.herdr.registered = vec![WorkspaceCheckout {
            workspace_id: "w2".into(),
            path: PathBuf::from("/repo.feature"),
        }];
        *engine.worktrunk.paths.borrow_mut() =
            [vec![PathBuf::from("/repo"), PathBuf::from("/repo.feature")]].into();
        engine.worktrunk.fail_snapshot = true;

        let outcome = engine.clean(&source()).unwrap();

        assert_eq!(outcome.close_workspace_ids, Vec::<String>::new());
        assert!(outcome.error.unwrap().contains("could not be verified"));
        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
    }

    #[test]
    fn clean_removes_integrated_checkouts_and_closes_only_their_visible_workspaces() {
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            false,
            false,
        );
        engine.herdr.registered = vec![
            WorkspaceCheckout {
                workspace_id: "w2".into(),
                path: PathBuf::from("/repo.feature"),
            },
            WorkspaceCheckout {
                workspace_id: "w3".into(),
                path: PathBuf::from("/repo.active"),
            },
        ];
        *engine.worktrunk.paths.borrow_mut() = [
            vec!["/repo", "/repo.feature", "/repo.unopened", "/repo.active"]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            vec![PathBuf::from("/repo"), PathBuf::from("/repo.active")],
        ]
        .into();

        let outcome = engine.clean(&source()).unwrap();

        assert_eq!(
            engine.worktrunk.calls.into_inner(),
            ["remove:/repo.feature", "remove:/repo.unopened"]
        );
        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
        assert_eq!(outcome.close_workspace_ids, ["w2"]);
        assert_eq!(outcome.completion_source.workspace_id, "w1");
        assert!(outcome.error.is_none());
        assert_eq!(outcome.message, "Cleaned 2 of 2 integrated worktrees");
    }

    #[test]
    fn clean_reconciles_removed_checkouts_even_when_removal_reports_failure() {
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            true,
        );
        engine.herdr.registered = vec![WorkspaceCheckout {
            workspace_id: "w2".into(),
            path: PathBuf::from("/repo.feature"),
        }];
        *engine.worktrunk.paths.borrow_mut() = [
            vec![
                PathBuf::from("/repo"),
                PathBuf::from("/repo.feature"),
                PathBuf::from("/repo.unopened"),
            ],
            vec![PathBuf::from("/repo"), PathBuf::from("/repo.unopened")],
        ]
        .into();

        let outcome = engine.clean(&source()).unwrap();

        assert_eq!(outcome.close_workspace_ids, ["w2"]);
        assert!(outcome.error.unwrap().contains("remove failed"));
        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
    }

    #[test]
    fn switch_opens_checkout_under_repository_parent() {
        let engine = engine(
            Checkout {
                branch: Some("feature".into()),
                path: PathBuf::from("/repo.feature"),
            },
            false,
            false,
        );

        engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert_eq!(
            engine.herdr.calls.into_inner(),
            ["open:w1:feature:/repo.feature:true"]
        );
    }

    #[test]
    fn create_opens_workspace_with_the_requested_focus() {
        for focus in [false, true] {
            let engine = engine(
                Checkout {
                    branch: Some("feature".into()),
                    path: PathBuf::from("/repo.feature"),
                },
                false,
                false,
            );
            engine
                .switch(
                    &source(),
                    &SwitchMode::Create {
                        branch: "feature".into(),
                        base: None,
                        focus,
                    },
                )
                .unwrap();
            assert_eq!(
                engine.herdr.calls.into_inner(),
                [format!("open:w1:feature:/repo.feature:{focus}")]
            );
        }
    }

    #[test]
    fn switch_to_primary_checkout_focuses_parent() {
        let engine = engine(
            Checkout {
                branch: Some("main".into()),
                path: PathBuf::from("/repo"),
            },
            true,
            false,
        );

        engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
    }

    #[test]
    fn cancelled_switch_leaves_herdr_unchanged() {
        let engine = Engine::new(
            FakeHerdr {
                context: Some(context(false)),
                ..Default::default()
            },
            FakeWorktrunk {
                checkout: None,
                merge_removed: true,
                calls: RefCell::default(),
                fail: false,
                paths: RefCell::default(),
                fail_snapshot: false,
                remove_on_switch: None,
            },
        );

        let outcome = engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert!(outcome.cancelled);
        assert_eq!(engine.herdr.calls.into_inner(), Vec::<String>::new());
    }

    fn removed_picker_worktree(engine: &mut Engine<FakeHerdr, FakeWorktrunk>) {
        engine.herdr.registered = vec![
            WorkspaceCheckout {
                workspace_id: "w3".into(),
                path: PathBuf::from("/repo.removed"),
            },
            WorkspaceCheckout {
                workspace_id: "w4".into(),
                path: PathBuf::from("/repo.retained"),
            },
            WorkspaceCheckout {
                workspace_id: "w5".into(),
                path: PathBuf::from("/repo.stale"),
            },
        ];
        *engine.worktrunk.paths.borrow_mut() = [
            vec![
                PathBuf::from("/repo"),
                PathBuf::from("/repo.removed"),
                PathBuf::from("/repo.retained"),
            ],
            vec![PathBuf::from("/repo"), PathBuf::from("/repo.retained")],
        ]
        .into();
    }

    #[test]
    fn picker_removal_matches_registered_symlink_paths_before_checkout_disappears() {
        let root = std::env::temp_dir().join(format!("wtherdr-alias-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let checkout = root.join("checkout");
        let alias = root.join("alias");
        std::fs::create_dir(&checkout).unwrap();
        std::os::unix::fs::symlink(&checkout, &alias).unwrap();
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            false,
            false,
        );
        engine.worktrunk.checkout = None;
        engine.worktrunk.remove_on_switch = Some(checkout.clone());
        engine.herdr.registered = vec![WorkspaceCheckout {
            workspace_id: "w3".into(),
            path: alias.clone(),
        }];
        *engine.worktrunk.paths.borrow_mut() =
            [vec![root.clone(), checkout], vec![root.clone()]].into();

        let outcome = engine.switch(&source(), &SwitchMode::Pick).unwrap();
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_dir(root).unwrap();

        assert_eq!(outcome.close_workspace_ids, ["w3"]);
    }

    #[test]
    fn cancelled_picker_reports_removed_checkout_without_an_open_workspace() {
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            false,
            false,
        );
        engine.worktrunk.checkout = None;
        *engine.worktrunk.paths.borrow_mut() = [
            vec![PathBuf::from("/repo"), PathBuf::from("/repo.unopened")],
            vec![PathBuf::from("/repo")],
        ]
        .into();

        let outcome = engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert!(!outcome.cancelled);
        assert_eq!(outcome.close_workspace_ids, Vec::<String>::new());
        assert!(outcome.message.contains("/repo.unopened"));
        assert!(engine.herdr.calls.borrow().is_empty());
    }

    #[test]
    fn cancelled_picker_reconciles_only_checkouts_proven_removed_during_the_action() {
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            false,
            false,
        );
        engine.worktrunk.checkout = None;
        removed_picker_worktree(&mut engine);

        let outcome = engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert!(!outcome.cancelled);
        assert_eq!(outcome.close_workspace_ids, ["w3"]);
        assert!(engine.herdr.calls.borrow().is_empty());
    }

    #[test]
    fn picker_failure_preserves_cleanup_for_confirmed_removals() {
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            false,
            true,
        );
        removed_picker_worktree(&mut engine);

        let outcome = engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert_eq!(outcome.close_workspace_ids, ["w3"]);
        assert!(outcome.error.unwrap().contains("switch failed"));
    }

    #[test]
    fn failed_workspace_open_preserves_cleanup_and_successful_checkout_details() {
        let mut engine = engine(
            Checkout {
                branch: Some("selected".into()),
                path: PathBuf::from("/repo.selected"),
            },
            false,
            false,
        );
        engine.herdr.fail_open = true;
        removed_picker_worktree(&mut engine);

        let outcome = engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert_eq!(outcome.close_workspace_ids, ["w3"]);
        assert!(outcome.message.contains("selected"));
        assert!(
            outcome
                .error
                .unwrap()
                .contains("Worktrunk switched to /repo.selected")
        );
    }

    #[test]
    fn failed_snapshot_preserves_selected_checkout_and_leaves_unverified_workspaces_open() {
        let mut engine = engine(
            Checkout {
                branch: Some("selected".into()),
                path: PathBuf::from("/repo.selected"),
            },
            false,
            false,
        );
        removed_picker_worktree(&mut engine);
        engine.worktrunk.paths.borrow_mut().truncate(1);
        engine.worktrunk.fail_snapshot = true;

        let outcome = engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert_eq!(outcome.close_workspace_ids, Vec::<String>::new());
        assert!(outcome.message.contains("selected"));
        assert!(outcome.error.unwrap().contains("snapshot failed"));
        assert_eq!(
            *engine.herdr.calls.borrow(),
            ["open:w1:selected:/repo.selected:true"]
        );
    }

    #[test]
    fn successful_remove_with_failed_parent_focus_reports_checkout_and_workspace_state() {
        let mut engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            false,
        );
        engine.herdr.fail_focus = true;

        let outcome = engine.remove(&source()).unwrap();

        assert!(outcome.message.contains("removed"));
        assert!(outcome.error.unwrap().contains("workspace w2 remains open"));
        assert_eq!(outcome.close_workspace_ids, Vec::<String>::new());
    }

    #[test]
    fn merge_defers_popup_owner_closure_until_completion() {
        let engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            false,
        );

        let outcome = engine.merge(&source()).unwrap();

        assert_eq!(outcome.close_workspace_ids, ["w2"]);
        assert_eq!(outcome.completion_source.workspace_id, "w1");
        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
    }

    #[test]
    fn remove_defers_popup_owner_closure_until_completion() {
        let engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            false,
        );

        let outcome = engine.remove(&source()).unwrap();

        assert_eq!(outcome.close_workspace_ids, ["w2"]);
        assert_eq!(outcome.completion_source.cwd, PathBuf::from("/repo"));
        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
    }

    #[test]
    fn merge_preserves_workspace_when_worktrunk_preserves_checkout() {
        let engine = Engine::new(
            FakeHerdr {
                context: Some(context(true)),
                ..Default::default()
            },
            FakeWorktrunk {
                checkout: None,
                merge_removed: false,
                calls: RefCell::default(),
                fail: false,
                paths: RefCell::default(),
                fail_snapshot: false,
                remove_on_switch: None,
            },
        );

        engine.merge(&source()).unwrap();

        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1"]);
    }

    #[test]
    fn remove_failure_leaves_workspace_open() {
        let engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            true,
        );

        assert!(engine.remove(&source()).is_err());
        assert_eq!(engine.herdr.calls.into_inner(), Vec::<String>::new());
    }

    #[test]
    fn merge_rejects_parent_workspace() {
        let engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            false,
            false,
        );

        let error = engine.merge(&source()).unwrap_err();

        assert!(error.to_string().contains("nested worktree workspace"));
        assert_eq!(engine.worktrunk.calls.into_inner(), Vec::<String>::new());
    }
}
