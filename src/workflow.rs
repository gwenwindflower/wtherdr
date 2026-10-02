use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::model::{Checkout, MergeOutcome, RepositoryContext, SourceWorkspace, SwitchMode};

pub trait Herdr {
    fn repository_context(&self, source: &SourceWorkspace) -> Result<RepositoryContext>;
    fn open_worktree(&self, parent_workspace_id: &str, checkout: &Checkout) -> Result<()>;
    fn focus_workspace(&self, workspace_id: &str) -> Result<()>;
    fn close_workspace(&self, workspace_id: &str) -> Result<()>;
}

pub trait Worktrunk {
    fn switch(&self, cwd: &Path, mode: &SwitchMode) -> Result<Option<Checkout>>;
    fn merge(&self, cwd: &Path) -> Result<MergeOutcome>;
    fn remove(&self, cwd: &Path) -> Result<()>;
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

    pub fn switch(&self, source: &SourceWorkspace, mode: &SwitchMode) -> Result<()> {
        let repository = self.herdr.repository_context(source)?;
        let Some(checkout) = self
            .worktrunk
            .switch(&source.cwd, mode)
            .context("Worktrunk could not switch worktrees")?
        else {
            return Ok(());
        };

        if same_path(&checkout.path, &repository.root_path) {
            self.herdr
                .focus_workspace(&repository.parent_workspace_id)
                .context("Worktrunk switched successfully, but Herdr could not focus the parent workspace")
        } else {
            self.herdr
                .open_worktree(&repository.parent_workspace_id, &checkout)
                .with_context(|| {
                    format!(
                        "Worktrunk switched to {}, but Herdr could not open it as a nested workspace",
                        checkout.path.display()
                    )
                })
        }
    }

    pub fn merge(&self, source: &SourceWorkspace) -> Result<()> {
        let repository = self.linked_repository(source, "merge")?;
        let outcome = self
            .worktrunk
            .merge(&source.cwd)
            .context("Worktrunk could not merge this worktree")?;
        if outcome.removed {
            self.finish_removal(source, &repository, "merged")
        } else {
            self.focus_parent(&repository, "merged")
        }
    }

    pub fn remove(&self, source: &SourceWorkspace) -> Result<()> {
        let repository = self.linked_repository(source, "remove")?;
        self.worktrunk
            .remove(&source.cwd)
            .context("Worktrunk could not remove this worktree")?;
        self.finish_removal(source, &repository, "removed")
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
    ) -> Result<()> {
        self.focus_parent(repository, operation)?;
        self.herdr
            .close_workspace(&source.workspace_id)
            .with_context(|| {
                format!(
                    "Worktrunk {operation} the checkout, but Herdr could not close workspace {}",
                    source.workspace_id
                )
            })
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
    }

    impl Herdr for FakeHerdr {
        fn repository_context(&self, _: &SourceWorkspace) -> Result<RepositoryContext> {
            self.context
                .clone()
                .ok_or_else(|| anyhow!("missing context"))
        }

        fn open_worktree(&self, parent: &str, checkout: &Checkout) -> Result<()> {
            self.calls.borrow_mut().push(format!(
                "open:{parent}:{}:{}",
                checkout.branch.as_deref().unwrap_or("detached"),
                checkout.path.display()
            ));
            Ok(())
        }

        fn focus_workspace(&self, workspace_id: &str) -> Result<()> {
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
    }

    impl Worktrunk for FakeWorktrunk {
        fn switch(&self, cwd: &Path, mode: &SwitchMode) -> Result<Option<Checkout>> {
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
            },
        )
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
            ["open:w1:feature:/repo.feature"]
        );
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
            },
        );

        engine.switch(&source(), &SwitchMode::Pick).unwrap();

        assert!(engine.herdr.calls.into_inner().is_empty());
    }

    #[test]
    fn merge_focuses_parent_before_closing_the_popup_owner() {
        let engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            false,
        );

        engine.merge(&source()).unwrap();

        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1", "close:w2"]);
    }

    #[test]
    fn remove_focuses_parent_before_closing_the_popup_owner() {
        let engine = engine(
            Checkout {
                branch: None,
                path: PathBuf::new(),
            },
            true,
            false,
        );

        engine.remove(&source()).unwrap();

        assert_eq!(engine.herdr.calls.into_inner(), ["focus:w1", "close:w2"]);
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
        assert!(engine.herdr.calls.into_inner().is_empty());
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
        assert!(engine.worktrunk.calls.into_inner().is_empty());
    }
}
