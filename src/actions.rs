use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub struct Actions {
    worktrunk: OsString,
    git: OsString,
}

impl Default for Actions {
    fn default() -> Self {
        Self::new("wt", "git")
    }
}

impl Actions {
    pub fn new(worktrunk: impl Into<OsString>, git: impl Into<OsString>) -> Self {
        Self {
            worktrunk: worktrunk.into(),
            git: git.into(),
        }
    }

    pub fn branches(&self, cwd: &Path) -> Result<Vec<String>> {
        let output = Command::new(&self.git)
            .arg("-C")
            .arg(cwd)
            .args([
                "for-each-ref",
                "--format=%(refname)%09%(symref)",
                "refs/heads",
                "refs/remotes",
            ])
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("could not list Git branches in {}", cwd.display()))?;
        if !output.status.success() {
            bail!(
                "could not list Git branches in {}: {}",
                cwd.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let refs = String::from_utf8(output.stdout).context("Git branch names were not UTF-8")?;
        let branches = refs
            .lines()
            .filter_map(|line| {
                let (reference, symbolic) = line.split_once('\t')?;
                if !symbolic.is_empty() {
                    return None;
                }
                reference
                    .strip_prefix("refs/heads/")
                    .or_else(|| reference.strip_prefix("refs/remotes/"))
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>();
        Ok(branches.into_iter().collect())
    }

    pub fn hooks(&self, cwd: &Path) -> Result<Vec<String>> {
        let output = self
            .command(cwd)
            .args(["hook", "show", "--format=json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| format!("could not list Worktrunk hooks in {}", cwd.display()))?;
        if !output.status.success() {
            bail!(
                "could not list Worktrunk hooks in {}: {}",
                cwd.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let hooks: Vec<Hook> = serde_json::from_slice(&output.stdout)
            .context("Worktrunk returned an unreadable hook list")?;
        let hooks = hooks
            .into_iter()
            .map(|hook| {
                if !HOOK_TYPES.contains(&hook.kind.as_str())
                    || !matches!(hook.source.as_str(), "user" | "project")
                {
                    bail!("Worktrunk returned an unsupported hook type or source");
                }
                Ok(format!(
                    "{} {}:{}",
                    hook.kind,
                    hook.source,
                    hook.name.as_deref().unwrap_or_default()
                ))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        Ok(hooks.into_iter().collect())
    }

    pub fn rebase(&self, cwd: &Path, target: &str) -> Result<()> {
        if target.trim().is_empty() {
            bail!("rebase needs a target branch");
        }
        let output =
            crate::transcript::run(self.command(cwd).args(["step", "rebase", "--", target]))
                .with_context(|| format!("could not rebase {} onto {target}", cwd.display()))?;
        let status = output.status;
        if !status.success() {
            bail!(
                "Worktrunk rebase of {} onto {target} exited with {status}. If a rebase is in progress, resolve the conflicts and run `git rebase --continue`, or run `git rebase --abort` in that worktree\n{}",
                cwd.display(),
                crate::transcript::diagnostic(&output.stderr)
            );
        }
        Ok(())
    }

    pub fn run_hook(&self, cwd: &Path, selection: &str) -> Result<()> {
        if !self.hooks(cwd)?.iter().any(|hook| hook == selection) {
            bail!(
                "Worktrunk hook {selection} is no longer configured in {}; open the hook picker again",
                cwd.display()
            );
        }
        let (kind, name) = selection
            .split_once(' ')
            .context("the selected Worktrunk hook is unreadable")?;
        let output =
            crate::transcript::run(self.command(cwd).args(["hook", kind, "--foreground", name]))
                .with_context(|| {
                    format!(
                        "could not run Worktrunk hook {selection} in {}",
                        cwd.display()
                    )
                })?;
        let status = output.status;
        if !status.success() {
            bail!(
                "Worktrunk hook {selection} in {} exited with {status}\n{}",
                cwd.display(),
                crate::transcript::diagnostic(&output.stderr)
            );
        }
        Ok(())
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::new(&self.worktrunk);
        command
            .arg("-C")
            .arg(cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        command
    }
}

#[derive(Deserialize)]
struct Hook {
    #[serde(rename = "type")]
    kind: String,
    source: String,
    name: Option<String>,
}

const HOOK_TYPES: &[&str] = &[
    "pre-switch",
    "post-switch",
    "pre-start",
    "post-start",
    "pre-commit",
    "post-commit",
    "pre-merge",
    "post-merge",
    "pre-remove",
    "post-remove",
];
