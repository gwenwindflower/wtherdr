use std::io::{self, IsTerminal};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command as Process, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::Value;

use wtherdr::actions::Actions;
use wtherdr::cli::{Cli, Command, PopupWorkflow};
use wtherdr::dialog;
use wtherdr::herdr::Api;
use wtherdr::invocation::source_workspace;
use wtherdr::model::{RepositoryContext, SourceWorkspace, SwitchMode, WorkflowOutcome};
use wtherdr::workflow::{Engine, Herdr, require_linked_worktree};
use wtherdr::worktrunk::CommandRunner;
use wtherdr::{completion, selector};

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
            focus,
        } => execute(workflow, branch, base, focus),
        Command::Handoff { workflow, finish } => {
            let source = workflow_source()?;
            let opened = Api::from_env().open_popup_when_free(
                if finish {
                    "complete"
                } else {
                    workflow.run_entrypoint()
                },
                &source,
                &carried_input(),
            );
            if finish && let Err(error) = opened {
                return recover_handoff(&error);
            }
            opened
        }
        Command::Finish { result } => finish_workflow(result.as_deref()),
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
            let Some(input) = dialog::create(&repository.repo_name)? else {
                return Ok(());
            };
            vec![
                ("WTHERDR_BRANCH", input.branch),
                ("WTHERDR_FOCUS", input.focus.to_string()),
            ]
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
        PopupWorkflow::Clean => {
            if !dialog::confirm(&repository.repo_name, "integrated worktrees", "clean")? {
                return Ok(());
            }
            Vec::new()
        }
        PopupWorkflow::Rebase => {
            let branches = Actions::default().branches(&source.cwd)?;
            let Some(target) =
                selector::choose(&repository.repo_name, "rebase", &branches, Some("main"))?
            else {
                return Ok(());
            };
            vec![("WTHERDR_BASE", target)]
        }
        PopupWorkflow::Hook => {
            let hooks = Actions::default().hooks(&source.cwd)?;
            let Some(hook) = selector::choose(&repository.repo_name, "run hook", &hooks, None)?
            else {
                return Ok(());
            };
            vec![("WTHERDR_HOOK", hook)]
        }
    };
    hand_off(workflow, &input, false)
}

fn hand_off(workflow: PopupWorkflow, input: &[(&str, String)], finish: bool) -> Result<()> {
    let mut opener = Process::new(std::env::current_exe()?);
    opener
        .arg("handoff")
        .arg(workflow.id())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if finish {
        opener.arg("--finish");
    }
    for (name, value) in input {
        opener.env(name, value);
    }
    opener
        .spawn()
        .with_context(|| format!("could not open the {} run popup", workflow.id()))?;
    Ok(())
}

fn execute(
    workflow: PopupWorkflow,
    branch: Option<String>,
    base: Option<String>,
    focus: Option<bool>,
) -> Result<()> {
    let source = workflow_source()?;
    let repository = Api::from_env().repository_context(&source)?;
    let completion_source = SourceWorkspace {
        workspace_id: repository.parent_workspace_id.clone(),
        cwd: repository.root_path.clone(),
    };
    let result = perform(workflow, &source, &repository, branch, base, focus);
    present(workflow, result, completion_source)
}

fn perform(
    workflow: PopupWorkflow,
    source: &SourceWorkspace,
    repository: &RepositoryContext,
    branch: Option<String>,
    base: Option<String>,
    focus: Option<bool>,
) -> Result<WorkflowOutcome> {
    let engine = engine();
    match workflow {
        PopupWorkflow::Create => {
            let branch = branch
                .or_else(|| std::env::var("WTHERDR_BRANCH").ok())
                .map(|branch| branch.trim().to_owned())
                .filter(|branch| !branch.is_empty())
                .context("create needs a branch name")?;
            let base = base.or_else(|| std::env::var("WTHERDR_BASE").ok());
            let focus = focus.map_or_else(
                || {
                    std::env::var("WTHERDR_FOCUS").map_or(Ok(false), |value| {
                        value
                            .parse::<bool>()
                            .context("WTHERDR_FOCUS must be true or false")
                    })
                },
                Ok,
            )?;
            engine.switch(
                source,
                &SwitchMode::Create {
                    branch,
                    base,
                    focus,
                },
            )
        }
        PopupWorkflow::Switch => engine.switch(source, &SwitchMode::Pick),
        PopupWorkflow::Merge => engine.merge(source),
        PopupWorkflow::Remove => engine.remove(source),
        PopupWorkflow::Clean => engine.clean(source),
        PopupWorkflow::Rebase => {
            let target = base
                .or_else(|| std::env::var("WTHERDR_BASE").ok())
                .filter(|target| !target.trim().is_empty())
                .unwrap_or_else(|| "main".into());
            Actions::default().rebase(&source.cwd, &target)?;
            Ok(WorkflowOutcome::completed(
                repository,
                format!(
                    "Rebased {} onto {target}",
                    repository.source_label.as_deref().unwrap_or("checkout")
                ),
            ))
        }
        PopupWorkflow::Hook => {
            let hook =
                std::env::var("WTHERDR_HOOK").context("hook needs a configured hook selection")?;
            Actions::default().run_hook(&source.cwd, &hook)?;
            Ok(WorkflowOutcome::completed(
                repository,
                format!("Finished {hook}"),
            ))
        }
    }
}

fn present(
    workflow: PopupWorkflow,
    result: Result<WorkflowOutcome>,
    completion_source: SourceWorkspace,
) -> Result<()> {
    if std::env::var_os("HERDR_PLUGIN_ENTRYPOINT_ID").is_none() {
        let outcome = result?;
        Api::from_env().finish_cleanup(&outcome)?;
        if let Some(error) = outcome.error {
            bail!("{error}");
        }
        if !outcome.cancelled {
            println!("{}", outcome.message);
        }
        return Ok(());
    }
    let outcome = result.unwrap_or_else(|error| WorkflowOutcome {
        message: format!("{} did not finish", workflow.id()),
        completion_source,
        close_workspace_ids: Vec::new(),
        cancelled: false,
        error: Some(format!("{error:#}")),
    });
    if outcome.cancelled {
        return Ok(());
    }
    if workflow == PopupWorkflow::Create && outcome.error.is_none() {
        return Api::from_env().finish_cleanup(&outcome);
    }
    hand_off(
        workflow,
        &[
            ("WTHERDR_OUTCOME", serde_json::to_string(&outcome)?),
            (
                "WTHERDR_SOURCE_WORKSPACE_ID",
                outcome.completion_source.workspace_id.clone(),
            ),
            (
                "WTHERDR_SOURCE_CWD",
                outcome.completion_source.cwd.to_string_lossy().into_owned(),
            ),
            ("WTHERDR_WORKFLOW", workflow.id().to_owned()),
        ],
        true,
    )
}

fn finish_workflow(path: Option<&std::path::Path>) -> Result<()> {
    let payload = path.map_or_else(
        || std::env::var("WTHERDR_OUTCOME").context("completion popup needs a workflow result"),
        |path| {
            std::fs::read_to_string(path)
                .with_context(|| format!("could not read workflow result {}", path.display()))
        },
    )?;
    let saved: serde_json::Value = serde_json::from_str(&payload)?;
    let outcome: WorkflowOutcome =
        serde_json::from_value(saved.get("outcome").unwrap_or(&saved).clone())?;
    let mut error = outcome.error.clone();
    let workflow = saved
        .get("workflow")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| std::env::var("WTHERDR_WORKFLOW").ok())
        .context("completion popup needs a workflow name")?;
    completion::show(&workflow, &outcome.message, &mut error, || {
        Api::from_env().finish_cleanup(&outcome)
    })
}

fn recover_handoff(error: &anyhow::Error) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut outcome: WorkflowOutcome = serde_json::from_str(&std::env::var("WTHERDR_OUTCOME")?)?;
    for workspace_id in std::mem::take(&mut outcome.close_workspace_ids) {
        let mut cleanup = outcome.clone();
        cleanup.close_workspace_ids = vec![workspace_id.clone()];
        if let Err(error) = Api::from_env().finish_cleanup(&cleanup) {
            let detail = format!("{error:#}");
            outcome.error = Some(
                outcome
                    .error
                    .map_or_else(|| detail.clone(), |error| format!("{error}\n{detail}")),
            );
            outcome.close_workspace_ids.push(workspace_id);
        }
    }
    let workflow = std::env::var("WTHERDR_WORKFLOW")?;
    let path = std::env::temp_dir().join(format!("wtherdr-result-{}.json", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(&serde_json::to_vec(
        &serde_json::json!({ "workflow": workflow, "outcome": outcome }),
    )?)?;
    let body = format!(
        "Could not open the {workflow} result popup: {error:#}. Result saved at {}. Read it with wtherdr finish --result {}",
        path.display(),
        path.display()
    );
    let _ = Process::new("herdr")
        .args([
            "notification",
            "show",
            "wtherdr result saved",
            "--body",
            &body,
            "--sound",
            "request",
        ])
        .status();
    bail!("{body}")
}

fn carried_input() -> Vec<(&'static str, String)> {
    [
        "WTHERDR_BRANCH",
        "WTHERDR_BASE",
        "WTHERDR_FOCUS",
        "WTHERDR_HOOK",
        "WTHERDR_OUTCOME",
        "WTHERDR_WORKFLOW",
    ]
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
