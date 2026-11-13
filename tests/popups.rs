use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Session {
    directory: PathBuf,
    requests: Arc<Mutex<Vec<Value>>>,
    popups: mpsc::Receiver<Value>,
    stop: Arc<AtomicBool>,
    reject_popup: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl Session {
    fn new(busy_once: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = PathBuf::from("/tmp").join(format!(
            "wtherdr-popups-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(directory.join("repo.feature")).unwrap();
        fs::create_dir(directory.join("repo")).unwrap();
        let listener = UnixListener::bind(directory.join("herdr.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reject_popup = Arc::new(AtomicBool::new(false));
        let (sender, popups) = mpsc::channel();
        let server_directory = directory.clone();
        let server_requests = Arc::clone(&requests);
        let server_stop = Arc::clone(&stop);
        let server_reject_popup = Arc::clone(&reject_popup);
        let server = thread::spawn(move || {
            let mut busy = busy_once;
            while !server_stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("mock Herdr accept failed: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                server_requests.lock().unwrap().push(request.clone());
                let response = match request["method"].as_str().unwrap() {
                    "worktree.list" => json!({"result": {
                        "source": {
                            "repo_name": "repo",
                            "repo_root": server_directory.join("repo"),
                            "source_workspace_id": "w1"
                        },
                        "worktrees": [
                            {"path": server_directory.join("repo"), "branch": "main", "open_workspace_id": "w1", "is_linked_worktree": false},
                            {"path": server_directory.join("repo.feature"), "branch": "feature", "open_workspace_id": "w2", "is_linked_worktree": true}
                        ]
                    }}),
                    "plugin.pane.open" if server_reject_popup.load(Ordering::Relaxed) => {
                        json!({"error": {"code": "unknown_entrypoint", "message": "result popup is unavailable"}})
                    }
                    "plugin.pane.open" if busy => {
                        busy = false;
                        json!({"error": {"code": "ui_busy", "message": "run popup is closing"}})
                    }
                    "plugin.pane.open" => {
                        sender.send(request["params"].clone()).unwrap();
                        json!({"result": {}})
                    }
                    "workspace.focus" | "workspace.close" | "worktree.open" => {
                        json!({"result": {}})
                    }
                    method => panic!("unexpected Herdr request: {method}"),
                };
                serde_json::to_writer(&mut stream, &response).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        Self {
            directory,
            requests,
            popups,
            stop,
            reject_popup,
            server: Some(server),
        }
    }

    fn executable(&self, name: &str, script: &str) {
        let path = self.directory.join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wtherdr"));
        let path = std::env::join_paths(
            std::iter::once(self.directory.clone())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        command
            .args(args)
            .env("PATH", path)
            .env("HERDR_SOCKET_PATH", self.directory.join("herdr.sock"))
            .env("HERDR_PLUGIN_ENTRYPOINT_ID", "test-run")
            .env("WTHERDR_SOURCE_WORKSPACE_ID", "w2")
            .env("WTHERDR_SOURCE_CWD", self.directory.join("repo.feature"))
            .env_remove("WTHERDR_OUTCOME")
            .env_remove("WTHERDR_HOOK")
            .env_remove("WTHERDR_BASE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn popup(&self) -> Value {
        self.popups
            .recv_timeout(Duration::from_secs(5))
            .expect("detached handoff did not open a popup")
    }

    fn methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["method"].as_str().unwrap().to_owned())
            .collect()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.server.take().unwrap().join().unwrap();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn run(mut command: Command, input: Option<&str>) -> Output {
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("wtherdr test process did not finish");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn removed_workspace_survives_until_the_completion_popup_handles_cleanup() {
    let session = Session::new(true);
    session.executable("wt", "printf '%s\\n' '{\"action\":\"removed\"}'");

    assert_success(&run(session.command(&["run", "remove"]), None));
    let popup = session.popup();
    assert_eq!(popup["entrypoint"], "complete");
    assert_eq!(popup["env"]["WTHERDR_WORKFLOW"], "remove");
    assert_eq!(popup["env"]["WTHERDR_SOURCE_WORKSPACE_ID"], "w1");
    assert_eq!(
        popup["env"]["WTHERDR_SOURCE_CWD"],
        session.directory.join("repo").to_str().unwrap()
    );
    let outcome: Value =
        serde_json::from_str(popup["env"]["WTHERDR_OUTCOME"].as_str().unwrap()).unwrap();
    assert_eq!(outcome["close_workspace_ids"], json!(["w2"]));
    let methods = session.methods();
    assert!(methods.iter().any(|method| method == "workspace.focus"));
    assert!(!methods.iter().any(|method| method == "workspace.close"));
    assert_eq!(
        methods
            .iter()
            .filter(|method| *method == "plugin.pane.open")
            .count(),
        2
    );

    let mut finish = session.command(&["finish"]);
    for (name, value) in popup["env"].as_object().unwrap() {
        finish.env(name, value.as_str().unwrap());
    }
    let output = run(finish, None);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("removed"));
    let requests = session.requests.lock().unwrap();
    let close = requests
        .iter()
        .find(|request| request["method"] == "workspace.close")
        .unwrap();
    assert_eq!(close["params"]["workspace_id"], "w2");
    assert_eq!(requests.last().unwrap()["method"], "workspace.focus");
    assert_eq!(requests.last().unwrap()["params"]["workspace_id"], "w1");
}

#[test]
fn clean_removes_integrated_checkouts_then_closes_nested_workspaces_on_main() {
    let session = Session::new(false);
    let checkout = session.directory.join("repo.feature");
    let listing = json!({"schema": 2, "items": [{
        "branch": "feature", "default_branch": {"integration": {"reason": "merge_adds_nothing"}},
        "worktree": {"path": checkout, "main": false, "detached": false, "changes": {
            "staged": false, "modified": false, "untracked": false,
            "renamed": false, "deleted": false, "conflicted": false
        }}
    }]});
    session.executable(
        "git",
        &format!(
            r"
printf 'worktree %s\0\0' '{}'
if test -d '{}'; then printf 'worktree %s\0\0' '{}'; fi
",
            session.directory.join("repo").display(),
            checkout.display(),
            checkout.display()
        ),
    );
    session.executable("wt", &format!(r#"
test "$1" = -C
cwd=$2
shift 2
case "$*" in
 '--config-set list.json-schema=2 list --format=json --no-progressive') printf '%s\n' '{listing}' ;;
 'remove --foreground --format=json') rmdir "$cwd"; printf '%s\n' '{{"kind":"worktree","branch_outcome":"deleted"}}' ;;
 *) exit 9 ;;
esac
"#));

    assert_success(&run(session.command(&["run", "clean"]), None));
    let popup = session.popup();
    let outcome: Value =
        serde_json::from_str(popup["env"]["WTHERDR_OUTCOME"].as_str().unwrap()).unwrap();
    assert_eq!(outcome["error"], Value::Null);
    assert_eq!(outcome["close_workspace_ids"], json!(["w2"]));
    assert_eq!(popup["env"]["WTHERDR_SOURCE_WORKSPACE_ID"], "w1");
    assert!(!checkout.exists());
    assert!(
        !session
            .methods()
            .iter()
            .any(|method| method == "workspace.close")
    );

    let mut finish = session.command(&["finish"]);
    for (name, value) in popup["env"].as_object().unwrap() {
        finish.env(name, value.as_str().unwrap());
    }
    assert_success(&run(finish, None));
    let requests = session.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request["method"] == "workspace.close"
                && request["params"]["workspace_id"] == "w2")
    );
    assert_eq!(requests.last().unwrap()["method"], "workspace.focus");
    assert_eq!(requests.last().unwrap()["params"]["workspace_id"], "w1");
}

#[test]
fn failed_remove_keeps_the_workspace_and_transfers_failure_to_completion() {
    let session = Session::new(false);
    session.executable(
        "wt",
        "printf '%s\\n' 'worktree has uncommitted changes' >&2\nexit 1",
    );

    assert_success(&run(session.command(&["run", "remove"]), None));
    let popup = session.popup();
    let outcome: Value =
        serde_json::from_str(popup["env"]["WTHERDR_OUTCOME"].as_str().unwrap()).unwrap();
    assert_eq!(outcome["close_workspace_ids"], json!([]));
    assert!(
        outcome["error"]
            .as_str()
            .unwrap()
            .contains("uncommitted changes")
    );
    assert!(
        !session
            .methods()
            .iter()
            .any(|method| method == "workspace.close")
    );
}

#[test]
fn successful_creation_opens_the_workspace_without_a_success_popup() {
    let session = Session::new(false);
    session.executable("wt", &format!("printf '%s\\n' '{}'", json!({
        "action": "created", "branch": "feature", "path": session.directory.join("repo.feature")
    })));

    assert_success(&run(
        session.command(&["run", "create", "--branch", "feature"]),
        None,
    ));

    assert!(
        session
            .methods()
            .iter()
            .any(|method| method == "worktree.open")
    );
    assert!(
        session
            .popups
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
    assert!(
        !session
            .methods()
            .iter()
            .any(|method| method == "plugin.pane.open")
    );
}

#[test]
fn failed_creation_still_presents_the_error_popup() {
    let session = Session::new(false);
    session.executable("wt", "printf '%s\\n' 'branch already exists' >&2\nexit 1");

    assert_success(&run(
        session.command(&["run", "create", "--branch", "feature"]),
        None,
    ));

    let popup = session.popup();
    assert_eq!(popup["entrypoint"], "complete");
    let outcome: Value =
        serde_json::from_str(popup["env"]["WTHERDR_OUTCOME"].as_str().unwrap()).unwrap();
    assert!(
        outcome["error"]
            .as_str()
            .unwrap()
            .contains("branch already exists")
    );
}

#[test]
fn switch_result_stays_on_the_selected_nested_workspace() {
    let session = Session::new(false);
    session.executable(
        "git",
        &format!(
            "printf 'worktree %s\\0\\0worktree %s\\0\\0' '{}' '{}'",
            session.directory.join("repo").display(),
            session.directory.join("repo.feature").display()
        ),
    );
    session.executable("wt", &format!("printf '%s\\n' '{}'", json!({
        "action": "switched", "branch": "feature", "path": session.directory.join("repo.feature")
    })));
    let mut command = session.command(&["run", "switch"]);
    command
        .env("WTHERDR_SOURCE_WORKSPACE_ID", "w1")
        .env("WTHERDR_SOURCE_CWD", session.directory.join("repo"));

    assert_success(&run(command, None));
    let popup = session.popup();
    assert_eq!(popup["env"]["WTHERDR_SOURCE_WORKSPACE_ID"], "w2");
    let requests = session.requests.lock().unwrap();
    let open = requests
        .iter()
        .find(|request| request["method"] == "worktree.open")
        .unwrap();
    assert_eq!(open["params"]["workspace_id"], "w1");
    assert_eq!(open["params"]["focus"], true);
    assert_eq!(
        open["params"]["path"],
        session.directory.join("repo.feature").to_str().unwrap()
    );
}

#[test]
fn switch_from_nested_workspace_to_main_focuses_main_through_completion() {
    let session = Session::new(false);
    session.executable(
        "git",
        &format!(
            "printf 'worktree %s\\0\\0' '{}'",
            session.directory.join("repo").display()
        ),
    );
    session.executable(
        "wt",
        &format!(
            "printf '%s\\n' '{}'",
            json!({
                "action": "switched", "branch": "main", "path": session.directory.join("repo")
            })
        ),
    );

    assert_success(&run(session.command(&["run", "switch"]), None));
    assert_eq!(session.popup()["env"]["WTHERDR_SOURCE_WORKSPACE_ID"], "w1");
    let requests = session.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request["method"] == "workspace.focus"
                && request["params"]["workspace_id"] == "w1")
    );
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "worktree.open")
    );
}

#[test]
fn rebase_filter_hands_the_existing_selected_branch_to_the_run_popup() {
    let session = Session::new(false);
    session.executable(
        "git",
        "printf 'refs/heads/main\\t\\nrefs/heads/release/stable\\t\\n'",
    );

    assert_success(&run(
        session.command(&["prompt", "rebase"]),
        Some("stable\n"),
    ));
    let popup = session.popup();
    assert_eq!(popup["entrypoint"], "rebase-run");
    assert_eq!(popup["env"]["WTHERDR_BASE"], "release/stable");
    assert_eq!(popup["env"]["WTHERDR_SOURCE_WORKSPACE_ID"], "w2");
}

#[test]
fn blank_rebase_input_selects_main() {
    let session = Session::new(false);
    session.executable(
        "git",
        "printf 'refs/heads/feature\\t\\nrefs/heads/main\\t\\n'",
    );

    assert_success(&run(session.command(&["prompt", "rebase"]), Some("\n")));
    assert_eq!(session.popup()["env"]["WTHERDR_BASE"], "main");
}

#[test]
fn hook_filter_hands_the_configured_hook_to_the_run_popup() {
    let session = Session::new(false);
    session.executable("wt", r#"printf '%s\n' '[{"type":"post-start","source":"user","name":"build"},{"type":"pre-merge","source":"project","name":"test"}]'"#);

    assert_success(&run(session.command(&["prompt", "hook"]), Some("build\n")));
    let popup = session.popup();
    assert_eq!(popup["entrypoint"], "hook-run");
    assert_eq!(popup["env"]["WTHERDR_HOOK"], "post-start user:build");
}

#[test]
fn rejected_completion_popup_preserves_the_result_and_finishes_cleanup() {
    let session = Session::new(false);
    session.reject_popup.store(true, Ordering::Relaxed);
    session.executable(
        "herdr",
        "printf '%s\\n' \"$@\" > \"$WTHERDR_NOTIFICATION_LOG\"",
    );
    let outcome = json!({
        "message": "Removed feature",
        "completion_source": {"workspace_id": "w1", "cwd": session.directory.join("repo")},
        "close_workspace_ids": ["w2"],
        "cancelled": false,
        "error": "post-hook failed after checkout removal"
    });
    let notification = session.directory.join("notification");
    let mut handoff = session.command(&["handoff", "remove", "--finish"]);
    handoff
        .env("WTHERDR_OUTCOME", outcome.to_string())
        .env("WTHERDR_WORKFLOW", "remove")
        .env("WTHERDR_NOTIFICATION_LOG", &notification);

    let output = run(handoff, None);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("result popup is unavailable"));
    let path = PathBuf::from(
        error
            .split_once("Result saved at ")
            .unwrap()
            .1
            .split_once(". Read it with ")
            .unwrap()
            .0,
    );
    let result = SavedResult(path);
    let saved: Value = serde_json::from_slice(&fs::read(&result.0).unwrap()).unwrap();
    assert_eq!(saved["workflow"], "remove");
    assert_eq!(saved["outcome"]["message"], "Removed feature");
    assert_eq!(saved["outcome"]["close_workspace_ids"], json!([]));
    assert_eq!(saved["outcome"]["error"], outcome["error"]);
    assert_eq!(
        fs::metadata(&result.0).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let notice = fs::read_to_string(notification).unwrap();
    assert!(notice.starts_with("notification\nshow\nwtherdr result saved\n--body\n"));
    assert!(notice.contains(&format!("wtherdr finish --result {}", result.0.display())));
    assert!(notice.ends_with("--sound\nrequest\n"));

    let mut replay = session.command(&["finish", "--result"]);
    replay.arg(&result.0);
    let output = run(replay, None);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Removed feature"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("post-hook failed after checkout removal")
    );
    let requests = session.requests.lock().unwrap();
    let closed: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "workspace.close")
        .collect();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0]["params"]["workspace_id"], "w2");
}

struct SavedResult(PathBuf);

impl Drop for SavedResult {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
