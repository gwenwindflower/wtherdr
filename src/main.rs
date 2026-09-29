use std::io::{self, IsTerminal};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command as Process, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::Value;

use wtherdr::cli::{Cli, Command, PopupWorkflow};
use wtherdr::dialog;
use wtherdr::herdr::Api;
use wtherdr::invocation::source_workspace;
use wtherdr::model::{SourceWorkspace, SwitchMode};
use wtherdr::workflow::{Engine, Herdr, require_linked_worktree};
use wtherdr::worktrunk::CommandRunner;

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if io::stderr().is_terminal() {
                eprintln!("\x1b[31merror:\x1b[0m {error:#}");
            } else {
                eprintln!("error: {error:#}");
            }
            if std::env::var_os("HERDR_PLUGIN_ENTRYPOINT_ID").is_some() {
                let _ = dialog::wait_for_dismiss();
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Popup { workflow } => {
            let source = action_source()?;
            Api::from_env().open_popup(workflow.action_entrypoint(), &source, &[])
        }
        Command::Prompt { workflow } => prompt(workflow),
        Command::Run {
            workflow,
            branch,
            base,
        } => execute(workflow, branch, base),
        Command::Handoff { workflow } => {
            let source = workflow_source()?;
            Api::from_env().open_popup_when_free(
                workflow.run_entrypoint(),
                &source,
                &carried_input(),
            )
        }
        Command::Status => {
            status();
            Ok(())
        }
    }
}

fn prompt(workflow: PopupWorkflow) -> Result<()> {
    let source = workflow_source()?;
    let repository = Api::from_env().repository_context(&source)?;
    let input = match workflow {
        PopupWorkflow::Create => {
            let Some(branch) = dialog::input(&repository.repo_name, "create")? else {
                return Ok(());
            };
            vec![("WTHERDR_BRANCH", branch)]
        }
        PopupWorkflow::Merge | PopupWorkflow::Remove => {
            require_linked_worktree(&repository, workflow.id())?;
            let checkout = repository
                .source_label
                .clone()
                .unwrap_or_else(|| source.cwd.display().to_string());
            if !dialog::confirm(&repository.repo_name, &checkout, workflow.id())? {
                return Ok(());
            }
            Vec::new()
        }
        PopupWorkflow::Switch => bail!("switch opens Worktrunk's picker without a dialog"),
    };
    hand_off(workflow, &input)
}

fn hand_off(workflow: PopupWorkflow, input: &[(&str, String)]) -> Result<()> {
    let mut opener = Process::new(std::env::current_exe()?);
    opener
        .arg("handoff")
        .arg(workflow.id())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    for (name, value) in input {
        opener.env(name, value);
    }
    opener
        .spawn()
        .with_context(|| format!("could not open the {} run popup", workflow.id()))?;
    Ok(())
}

fn execute(workflow: PopupWorkflow, branch: Option<String>, base: Option<String>) -> Result<()> {
    let source = workflow_source()?;
    let engine = engine();
    match workflow {
        PopupWorkflow::Create => {
            let branch = branch
                .or_else(|| std::env::var("WTHERDR_BRANCH").ok())
                .map(|branch| branch.trim().to_owned())
                .filter(|branch| !branch.is_empty())
                .context("create needs a branch name")?;
            let base = base.or_else(|| std::env::var("WTHERDR_BASE").ok());
            engine.switch(&source, &SwitchMode::Create { branch, base })
        }
        PopupWorkflow::Switch => engine.switch(&source, &SwitchMode::Pick),
        PopupWorkflow::Merge => engine.merge(&source),
        PopupWorkflow::Remove => engine.remove(&source),
    }
}

fn carried_input() -> Vec<(&'static str, String)> {
    ["WTHERDR_BRANCH", "WTHERDR_BASE"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|value| (name, value)))
        .collect()
}

fn engine() -> Engine<Api, CommandRunner> {
    Engine::new(Api::from_env(), CommandRunner::default())
}

fn action_source() -> Result<SourceWorkspace> {
    let context = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").context(
        "HERDR_PLUGIN_CONTEXT_JSON is missing; run popup commands through a Herdr plugin action",
    )?;
    let workspace_id = std::env::var("HERDR_WORKSPACE_ID").ok();
    source_workspace(workspace_id.as_deref(), &context)
}

fn workflow_source() -> Result<SourceWorkspace> {
    let workspace_id = std::env::var("WTHERDR_SOURCE_WORKSPACE_ID")
        .or_else(|_| std::env::var("HERDR_WORKSPACE_ID"))
        .context("no Herdr workspace is associated with this command")?;
    let cwd = std::env::var_os("WTHERDR_SOURCE_CWD")
        .map(PathBuf::from)
        .map_or_else(std::env::current_dir, Ok)?;
    Ok(SourceWorkspace { workspace_id, cwd })
}

fn status() {
    match CommandRunner::default().version() {
        Ok(version) => println!("worktrunk  {version}"),
        Err(error) => println!("worktrunk  unavailable: {error}"),
    }
    match Api::from_env().ping() {
        Ok(pong) => {
            let version = pong.get("version").and_then(Value::as_str).unwrap_or("?");
            let protocol = pong.get("protocol").and_then(Value::as_i64).unwrap_or(0);
            println!("herdr      {version} (protocol {protocol})");
        }
        Err(error) => println!("herdr      unavailable: {error}"),
    }
}
