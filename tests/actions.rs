use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use wtherdr::actions::Actions;

struct Directory {
    path: PathBuf,
    _process_guard: MutexGuard<'static, ()>,
}

impl Directory {
    fn new() -> Self {
        static PROCESS_LOCK: Mutex<()> = Mutex::new(());
        let process_guard = PROCESS_LOCK.lock().unwrap();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wtherdr-actions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self {
            path,
            _process_guard: process_guard,
        }
    }

    fn executable(&self, script: &str) -> PathBuf {
        let path = self.path.join("wt");
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git(cwd: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn branches_include_local_and_remote_refs_without_symbolic_aliases() {
    let directory = Directory::new();
    git(
        &directory.path,
        &["init", "--initial-branch=main", "--quiet"],
    );
    git(
        &directory.path,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "--no-gpg-sign",
            "-m",
            "initial",
            "--quiet",
        ],
    );
    git(&directory.path, &["branch", "feature/api"]);
    git(
        &directory.path,
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(
        &directory.path,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );

    assert_eq!(
        Actions::default().branches(&directory.path).unwrap(),
        ["feature/api", "main", "origin/main"]
    );
}

#[test]
fn hooks_offer_named_and_unnamed_commands_from_both_sources() {
    let directory = Directory::new();
    let binary = directory.executable(r#"
test "$1" = '-C'
shift 2
test "$*" = 'hook show --format=json'
printf '%s\n' '[{"type":"pre-merge","source":"project","name":"gate"},{"type":"post-start","source":"user","name":null},{"type":"pre-merge","source":"user","name":"gate"},{"type":"pre-merge","source":"project","name":"gate"}]'
"#);

    assert_eq!(
        Actions::new(binary, "git").hooks(&directory.path).unwrap(),
        [
            "post-start user:",
            "pre-merge project:gate",
            "pre-merge user:gate"
        ]
    );
}

#[test]
fn hook_selection_runs_only_the_selected_source_and_waits_for_completion() {
    let directory = Directory::new();
    let binary = directory.executable(r#"
test "$1" = '-C'
directory=$2
shift 2
case "$*" in
 'hook show --format=json') printf '%s\n' '[{"type":"post-start","source":"user","name":"build"}]' ;;
 'hook post-start --foreground user:build') printf 'built\n' > "$directory/built" ;;
 *) exit 9 ;;
esac
"#);

    Actions::new(binary, "git")
        .run_hook(&directory.path, "post-start user:build")
        .unwrap();
    assert_eq!(
        fs::read_to_string(directory.path.join("built")).unwrap(),
        "built\n"
    );
}

#[test]
fn failed_hook_reports_the_selected_command_and_worktree() {
    let directory = Directory::new();
    let binary = directory.executable(r#"
shift 2
case "$*" in
 'hook show --format=json') printf '%s\n' '[{"type":"pre-merge","source":"project","name":"test"}]' ;;
 'hook pre-merge --foreground project:test') exit 5 ;;
 *) exit 9 ;;
esac
"#);
    let error = Actions::new(binary, "git")
        .run_hook(&directory.path, "pre-merge project:test")
        .unwrap_err()
        .to_string();
    assert!(error.contains("pre-merge project:test"));
    assert!(error.contains(&directory.path.display().to_string()));
    assert!(error.contains('5'));
}

#[test]
fn stale_or_invented_hook_selection_does_not_execute() {
    let directory = Directory::new();
    let binary = directory.executable("test \"$4\" = show\nprintf '[]\\n'");
    let error = Actions::new(binary, "git")
        .run_hook(&directory.path, "post-start user:missing")
        .unwrap_err();
    assert!(error.to_string().contains("no longer configured"));
}

#[test]
fn rebase_passes_the_target_as_one_argument_without_approval_bypasses() {
    let directory = Directory::new();
    let binary = directory.executable(
        r#"
test "$1" = '-C'
shift 2
test "$#" = 4
test "$1" = step
test "$2" = rebase
test "$3" = --
test "$4" = origin/main
"#,
    );
    Actions::new(binary, OsStr::new("git"))
        .rebase(&directory.path, "origin/main")
        .unwrap();
}

#[test]
fn rebase_failure_explains_where_to_resolve_or_abort() {
    let directory = Directory::new();
    let binary = directory.executable("exit 1");
    let error = Actions::new(binary, "git")
        .rebase(&directory.path, "main")
        .unwrap_err()
        .to_string();
    assert!(error.contains(&directory.path.display().to_string()));
    assert!(error.contains("main"));
    assert!(error.contains("git rebase --continue"));
    assert!(error.contains("git rebase --abort"));
}
