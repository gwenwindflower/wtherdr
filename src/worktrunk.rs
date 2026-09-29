use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::model::{Checkout, MergeOutcome, SwitchMode};
use crate::workflow::Worktrunk;

pub struct CommandRunner {
    binary: OsString,
}

struct DirectiveFiles {
    cd: PathBuf,
    exec: PathBuf,
}

impl DirectiveFiles {
    fn create() -> Result<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        for _ in 0..100 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let stem = format!("wtherdr-{}-{id}", std::process::id());
            let cd = std::env::temp_dir().join(format!("{stem}-cd"));
            let exec = std::env::temp_dir().join(format!("{stem}-exec"));
            if OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&cd)
                .is_err()
            {
                continue;
            }
            if let Err(error) = OpenOptions::new().write(true).create_new(true).open(&exec) {
                let _ = fs::remove_file(&cd);
                return Err(error).context("could not create Worktrunk directive file");
            }
            return Ok(Self { cd, exec });
        }

        bail!("could not reserve temporary files for Worktrunk shell directives")
    }

    fn cd_path(&self) -> &Path {
        &self.cd
    }

    fn exec_path(&self) -> &Path {
        &self.exec
    }
}

impl Drop for DirectiveFiles {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.cd);
        let _ = fs::remove_file(&self.exec);
    }
}

impl CommandRunner {
    pub fn new(binary: impl Into<OsString>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn version(&self) -> Result<String> {
        let output = Command::new(&self.binary)
            .arg("--version")
            .output()
            .with_context(|| {
                format!(
                    "could not start Worktrunk with {}",
                    self.binary.to_string_lossy()
                )
            })?;
        if !output.status.success() {
            bail!("Worktrunk version check exited with {}", output.status);
        }
        let version = String::from_utf8(output.stdout)
            .context("Worktrunk version output was not UTF-8")?
            .trim()
            .to_owned();
        if version.is_empty() {
            bail!("Worktrunk version check returned no version");
        }
        Ok(version)
    }

    fn switch_args(cwd: &Path, mode: &SwitchMode) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("-C"),
            cwd.as_os_str().to_owned(),
            OsString::from("switch"),
        ];
        match mode {
            SwitchMode::Pick => {}
            SwitchMode::Create { branch, base } => {
                args.push(OsString::from("--create"));
                args.push(OsString::from(branch));
                if let Some(base) = base {
                    args.push(OsString::from("--base"));
                    args.push(OsString::from(base));
                }
            }
        }
        args.push(OsString::from("--no-cd"));
        args.push(OsString::from("--format=json"));
        args
    }

    fn parse_checkout(stdout: &[u8]) -> Result<Checkout> {
        let result: SwitchResult = serde_json::from_slice(stdout)
            .context("Worktrunk returned an unreadable switch result")?;
        let path = result
            .path
            .filter(|path| !path.as_os_str().is_empty())
            .context("Worktrunk switch result did not include a worktree path")?;
        Ok(Checkout {
            branch: result.branch,
            path,
        })
    }

    fn parse_switch(stdout: &[u8]) -> Result<Option<Checkout>> {
        if stdout.is_empty() {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_slice(stdout)
            .context("Worktrunk returned an unreadable switch result")?;
        if value.get("action").and_then(serde_json::Value::as_str) == Some("cancelled") {
            return Ok(None);
        }
        Self::parse_checkout(stdout).map(Some)
    }

    fn command(&self, args: &[OsString], directives: Option<&DirectiveFiles>) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(directives) = directives {
            command
                .env("WORKTRUNK_DIRECTIVE_CD_FILE", directives.cd_path())
                .env("WORKTRUNK_DIRECTIVE_EXEC_FILE", directives.exec_path());
        }
        command
    }

    fn output(&self, args: &[OsString], with_directives: bool) -> Result<Output> {
        let directives = with_directives
            .then(DirectiveFiles::create)
            .transpose()
            .context("could not prepare Worktrunk shell integration")?;
        self.command(args, directives.as_ref())
            .output()
            .with_context(|| {
                format!(
                    "could not start Worktrunk with {}",
                    self.binary.to_string_lossy()
                )
            })
    }

    fn run_json(&self, args: &[OsString], operation: &str) -> Result<Vec<u8>> {
        let output = self.output(args, true)?;
        if !output.status.success() {
            bail!("Worktrunk {operation} exited with {}", output.status);
        }
        if output.stdout.is_empty() {
            bail!("Worktrunk {operation} returned no machine-readable result");
        }
        Ok(output.stdout)
    }
}

#[derive(Deserialize)]
struct SwitchResult {
    branch: Option<String>,
    path: Option<std::path::PathBuf>,
}

#[derive(Deserialize)]
struct MergeResult {
    removed: bool,
}

impl Default for CommandRunner {
    fn default() -> Self {
        Self::new("wt")
    }
}

impl Worktrunk for CommandRunner {
    fn switch(&self, cwd: &Path, mode: &SwitchMode) -> Result<Option<Checkout>> {
        let output = self.output(&Self::switch_args(cwd, mode), false)?;
        if output.status.code() == Some(130) {
            return Ok(None);
        }
        if !output.status.success() {
            bail!("Worktrunk switch exited with {}", output.status);
        }
        Self::parse_switch(&output.stdout)
    }

    fn merge(&self, cwd: &Path) -> Result<MergeOutcome> {
        let args = [
            OsString::from("-C"),
            cwd.as_os_str().to_owned(),
            OsString::from("merge"),
            OsString::from("--format=json"),
        ];
        let stdout = self.run_json(&args, "merge")?;
        let result: MergeResult = serde_json::from_slice(&stdout)
            .context("Worktrunk returned an unreadable merge result")?;
        Ok(MergeOutcome {
            removed: result.removed,
        })
    }

    fn remove(&self, cwd: &Path) -> Result<()> {
        let args = [
            OsString::from("-C"),
            cwd.as_os_str().to_owned(),
            OsString::from("remove"),
            OsString::from("--foreground"),
            OsString::from("--format=json"),
        ];
        let stdout = self.run_json(&args, "remove")?;
        serde_json::from_slice::<serde_json::Value>(&stdout)
            .context("Worktrunk returned an unreadable remove result")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use super::*;

    fn strings(values: Vec<OsString>) -> Vec<String> {
        values
            .into_iter()
            .map(|value| value.into_string().unwrap())
            .collect()
    }

    #[test]
    fn parses_current_switch_result() {
        let checkout = CommandRunner::parse_checkout(
            br#"{"action":"created","branch":"feature/api","path":"/repo.feature-api"}"#,
        )
        .unwrap();

        assert_eq!(
            checkout,
            Checkout {
                branch: Some("feature/api".into()),
                path: PathBuf::from("/repo.feature-api"),
            }
        );
    }

    #[test]
    fn create_uses_worktrunk_lifecycle_and_machine_result() {
        let args = CommandRunner::switch_args(
            Path::new("/repo"),
            &SwitchMode::Create {
                branch: "feature/api".into(),
                base: Some("@".into()),
            },
        );

        assert_eq!(
            strings(args),
            [
                "-C",
                "/repo",
                "switch",
                "--create",
                "feature/api",
                "--base",
                "@",
                "--no-cd",
                "--format=json",
            ]
        );
    }

    #[test]
    fn switch_keeps_native_picker() {
        let args = CommandRunner::switch_args(Path::new("/repo"), &SwitchMode::Pick);

        assert_eq!(
            strings(args),
            ["-C", "/repo", "switch", "--no-cd", "--format=json"]
        );
    }

    #[test]
    fn worktrunk_receives_ephemeral_shell_directive_files() {
        let directives = DirectiveFiles::create().unwrap();
        let cd_path = directives.cd_path().to_owned();
        let exec_path = directives.exec_path().to_owned();
        let command = CommandRunner::new("wt").command(&[], Some(&directives));

        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == OsStr::new("WORKTRUNK_DIRECTIVE_CD_FILE"))
                .and_then(|(_, value)| value),
            Some(cd_path.as_os_str())
        );
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == OsStr::new("WORKTRUNK_DIRECTIVE_EXEC_FILE"))
                .and_then(|(_, value)| value),
            Some(exec_path.as_os_str())
        );
        assert!(cd_path.exists());
        assert!(exec_path.exists());

        drop(directives);

        assert!(!cd_path.exists());
        assert!(!exec_path.exists());
    }

    #[test]
    fn missing_switch_path_has_actionable_error() {
        let error = CommandRunner::parse_checkout(br#"{"action":"cancelled"}"#).unwrap_err();
        assert!(error.to_string().contains("path"));
    }

    #[test]
    fn cancelled_switch_is_not_an_error() {
        assert_eq!(
            CommandRunner::parse_switch(br#"{"action":"cancelled"}"#).unwrap(),
            None
        );
        assert_eq!(CommandRunner::parse_switch(b"").unwrap(), None);
    }

    #[test]
    fn merge_reports_whether_worktrunk_removed_the_checkout() {
        let result: MergeResult =
            serde_json::from_slice(br#"{"branch":"feature","removed":true,"target":"main"}"#)
                .unwrap();

        assert!(result.removed);
    }
}
