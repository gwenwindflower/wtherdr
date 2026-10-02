use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::model::{Checkout, RepositoryContext, SourceWorkspace};
use crate::workflow::Herdr;

pub fn repository_context(result: &Value, source: &SourceWorkspace) -> Result<RepositoryContext> {
    let listing: WorktreeList = serde_json::from_value(result.clone())
        .context("Herdr returned an unreadable worktree list")?;
    let source_worktree = listing
        .worktrees
        .iter()
        .find(|worktree| worktree.open_workspace_id.as_deref() == Some(&source.workspace_id));
    let source_is_linked_worktree = match source_worktree {
        Some(worktree) => worktree.is_linked_worktree,
        None if source.workspace_id == listing.source.source_workspace_id => false,
        None => {
            bail!(
                "Herdr workspace {} is not registered in the repository worktree group",
                source.workspace_id
            )
        }
    };
    let source_label = source_worktree.and_then(|worktree| {
        worktree
            .branch
            .clone()
            .or_else(|| worktree.label.clone())
            .filter(|label| !label.is_empty())
    });
    Ok(RepositoryContext {
        parent_workspace_id: listing.source.source_workspace_id,
        root_path: listing.source.repo_root,
        repo_name: listing.source.repo_name,
        source_label,
        source_is_linked_worktree,
    })
}

pub fn popup_request(
    plugin_id: &str,
    entrypoint: &str,
    source: &SourceWorkspace,
    input: &[(&str, String)],
) -> Value {
    let mut env = json!({
        "WTHERDR_SOURCE_WORKSPACE_ID": source.workspace_id,
        "WTHERDR_SOURCE_CWD": source.cwd,
    });
    for (name, value) in input {
        env[*name] = Value::String(value.clone());
    }
    json!({
        "plugin_id": plugin_id,
        "entrypoint": entrypoint,
        "placement": "popup",
        "focus": true,
        "env": env,
    })
}

#[derive(Deserialize)]
struct WorktreeList {
    source: WorktreeSource,
    worktrees: Vec<WorktreeRecord>,
}

#[derive(Deserialize)]
struct WorktreeSource {
    repo_name: String,
    repo_root: PathBuf,
    source_workspace_id: String,
}

#[derive(Deserialize)]
struct WorktreeRecord {
    branch: Option<String>,
    label: Option<String>,
    open_workspace_id: Option<String>,
    is_linked_worktree: bool,
}

#[derive(Clone, Debug)]
pub struct RequestError {
    pub method: String,
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Herdr {} failed ({}): {}",
            self.method, self.code, self.message
        )
    }
}

impl std::error::Error for RequestError {}

pub fn is_modal_conflict(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<RequestError>()
        .is_some_and(|failure| failure.code == "ui_busy")
}

#[derive(Clone, Debug)]
struct Client {
    socket: PathBuf,
}

impl Client {
    fn from_env() -> Self {
        let socket = std::env::var_os("HERDR_SOCKET_PATH")
            .map_or_else(|| config_home().join("herdr/herdr.sock"), PathBuf::from);
        Self { socket }
    }

    fn call(&self, method: &str, params: &Value) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.socket)
            .with_context(|| format!("connecting to Herdr at {}", self.socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let request = json!({ "id": "wtherdr", "method": method, "params": params });
        serde_json::to_writer(&mut stream, &request)?;
        stream.write_all(b"\n")?;

        let mut line = String::new();
        BufReader::new(stream)
            .read_line(&mut line)
            .with_context(|| format!("reading Herdr {method} response"))?;
        if line.is_empty() {
            bail!("Herdr closed the connection without answering {method}");
        }
        let response: Value = serde_json::from_str(&line)
            .with_context(|| format!("parsing Herdr {method} response"))?;
        if let Some(error) = response.get("error") {
            return Err(RequestError {
                method: method.to_owned(),
                code: error
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_error")
                    .to_owned(),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Herdr rejected the request")
                    .to_owned(),
            }
            .into());
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("Herdr {method} response did not include a result"))
    }
}

pub struct Api {
    client: Client,
    plugin_id: String,
}

impl Api {
    pub fn from_env() -> Self {
        Self {
            client: Client::from_env(),
            plugin_id: std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "wtherdr".into()),
        }
    }
}

impl Herdr for Api {
    fn repository_context(&self, source: &SourceWorkspace) -> Result<RepositoryContext> {
        let result = self
            .client
            .call("worktree.list", &json!({ "cwd": absolute(&source.cwd)? }))?;
        repository_context(&result, source)
    }

    fn open_worktree(&self, parent: &str, checkout: &Checkout, focus: bool) -> Result<()> {
        let mut params = json!({
            "workspace_id": parent,
            "path": absolute(&checkout.path)?,
            "focus": focus,
        });
        if let Some(branch) = &checkout.branch {
            params["label"] = Value::String(branch.clone());
        }
        self.client.call("worktree.open", &params)?;
        Ok(())
    }

    fn focus_workspace(&self, workspace_id: &str) -> Result<()> {
        self.client
            .call("workspace.focus", &json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<()> {
        self.client
            .call("workspace.close", &json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }
}

impl Api {
    pub fn open_popup(
        &self,
        entrypoint: &str,
        source: &SourceWorkspace,
        input: &[(&str, String)],
    ) -> Result<()> {
        self.client.call(
            "plugin.pane.open",
            &popup_request(&self.plugin_id, entrypoint, source, input),
        )?;
        Ok(())
    }

    pub fn open_popup_when_free(
        &self,
        entrypoint: &str,
        source: &SourceWorkspace,
        input: &[(&str, String)],
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.open_popup(entrypoint, source, input) {
                Err(error) if is_modal_conflict(&error) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(40));
                }
                result => return result,
            }
        }
    }

    pub fn ping(&self) -> Result<Value> {
        self.client.call("ping", &json!({}))
    }
}

fn absolute(path: &Path) -> Result<&Path> {
    if path.is_absolute() {
        Ok(path)
    } else {
        bail!("Herdr API paths must be absolute: {}", path.display())
    }
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn source(workspace_id: &str, cwd: &str) -> SourceWorkspace {
        SourceWorkspace {
            workspace_id: workspace_id.into(),
            cwd: PathBuf::from(cwd),
        }
    }

    #[test]
    fn nested_workspace_resolves_repository_parent() {
        let result = json!({
            "source": {
                "repo_name": "repo",
                "repo_root": "/repo",
                "source_workspace_id": "w1"
            },
            "worktrees": [
                {
                    "path": "/repo",
                    "label": "main",
                    "open_workspace_id": "w1",
                    "is_linked_worktree": false
                },
                {
                    "path": "/repo.feature",
                    "branch": "feature",
                    "label": "feature",
                    "open_workspace_id": "w2",
                    "is_linked_worktree": true
                }
            ]
        });

        assert_eq!(
            repository_context(&result, &source("w2", "/repo.feature")).unwrap(),
            RepositoryContext {
                parent_workspace_id: "w1".into(),
                root_path: PathBuf::from("/repo"),
                repo_name: "repo".into(),
                source_label: Some("feature".into()),
                source_is_linked_worktree: true,
            }
        );
    }

    #[test]
    fn popup_carries_source_context_and_leaves_geometry_to_the_manifest() {
        let request = popup_request("wtherdr", "switch-run", &source("w2", "/repo.feature"), &[]);

        assert_eq!(request["plugin_id"], "wtherdr");
        assert_eq!(request["entrypoint"], "switch-run");
        assert_eq!(request["placement"], "popup");
        assert!(request.get("width").is_none());
        assert!(request.get("height").is_none());
        assert!(request.get("cwd").is_none());
        assert_eq!(request["env"]["WTHERDR_SOURCE_WORKSPACE_ID"], "w2");
        assert_eq!(request["env"]["WTHERDR_SOURCE_CWD"], "/repo.feature");
        assert!(request["env"].get("PATH").is_none());
    }

    #[test]
    fn popup_carries_the_workflow_input_collected_by_the_dialog() {
        let request = popup_request(
            "wtherdr",
            "create-run",
            &source("w2", "/repo"),
            &[("WTHERDR_BRANCH", "feature/api".into())],
        );

        assert_eq!(request["env"]["WTHERDR_BRANCH"], "feature/api");
    }

    #[test]
    fn an_open_modal_is_a_retryable_conflict() {
        let busy: anyhow::Error = RequestError {
            method: "plugin.pane.open".into(),
            code: "ui_busy".into(),
            message: "another modal is active".into(),
        }
        .into();
        let missing: anyhow::Error = RequestError {
            method: "plugin.pane.open".into(),
            code: "not_found".into(),
            message: "unknown entrypoint".into(),
        }
        .into();

        assert!(is_modal_conflict(&busy));
        assert!(!is_modal_conflict(&missing));
    }
}
