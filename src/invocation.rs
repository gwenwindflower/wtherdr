use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::model::SourceWorkspace;

pub fn source_workspace(workspace_id: Option<&str>, context_json: &str) -> Result<SourceWorkspace> {
    let context: PluginContext = serde_json::from_str(context_json)
        .context("HERDR_PLUGIN_CONTEXT_JSON is not valid JSON")?;
    let workspace_id = workspace_id
        .map(str::to_owned)
        .or(context.workspace_id)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("the Herdr action did not include a workspace id"))?;
    let cwd = context
        .workspace_cwd
        .or(context.focused_pane_cwd)
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("the Herdr action did not include a workspace directory"))?;
    Ok(SourceWorkspace { workspace_id, cwd })
}

#[derive(Deserialize)]
struct PluginContext {
    workspace_id: Option<String>,
    workspace_cwd: Option<std::path::PathBuf>,
    focused_pane_cwd: Option<std::path::PathBuf>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn invocation_uses_workspace_root_instead_of_focused_subdirectory() {
        let source = source_workspace(
            Some("w2"),
            r#"{
                "workspace_cwd": "/repo.feature",
                "focused_pane_cwd": "/repo.feature/crates/app"
            }"#,
        )
        .unwrap();

        assert_eq!(
            source,
            SourceWorkspace {
                workspace_id: "w2".into(),
                cwd: PathBuf::from("/repo.feature"),
            }
        );
    }

    #[test]
    fn invocation_requires_workspace_context() {
        let error = source_workspace(None, r#"{"focused_pane_cwd":"/repo"}"#).unwrap_err();
        assert!(error.to_string().contains("workspace"));
    }
}
