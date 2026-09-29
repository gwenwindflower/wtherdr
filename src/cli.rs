use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "wtherdr",
    version,
    about = "Worktrunk worktree workflows as native Herdr workspaces",
    long_about = "wtherdr runs Worktrunk's real switch, create, merge, and remove workflows in \
Herdr modal popups. Worktrunk owns Git worktrees and lifecycle hooks; Herdr opens the resulting \
checkouts as nested workspaces and reconciles them after cleanup."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open a Worktrunk workflow in its Herdr modal popup
    Popup {
        #[arg(value_enum)]
        workflow: PopupWorkflow,
    },
    /// Ask for the workflow's input in a dialog popup, then open its run popup
    Prompt {
        #[arg(value_enum)]
        workflow: PopupWorkflow,
    },
    /// Run the Worktrunk workflow and reconcile Herdr's workspaces
    Run {
        #[arg(value_enum)]
        workflow: PopupWorkflow,
        /// Branch to create; read from the dialog popup when omitted
        #[arg(long)]
        branch: Option<String>,
        /// Base branch or Worktrunk shortcut such as @, ^, or pr:123
        #[arg(long)]
        base: Option<String>,
    },
    /// Open a workflow's run popup as soon as its dialog popup closes
    #[command(hide = true)]
    Handoff {
        #[arg(value_enum)]
        workflow: PopupWorkflow,
    },
    /// Show Herdr and Worktrunk connectivity
    Status,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum PopupWorkflow {
    Create,
    Switch,
    Merge,
    Remove,
}

impl PopupWorkflow {
    pub fn id(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Switch => "switch",
            Self::Merge => "merge",
            Self::Remove => "remove",
        }
    }

    pub fn dialog_entrypoint(self) -> Option<&'static str> {
        match self {
            Self::Switch => None,
            _ => Some(self.id()),
        }
    }

    pub fn run_entrypoint(self) -> &'static str {
        match self {
            Self::Create => "create-run",
            Self::Switch => "switch-run",
            Self::Merge => "merge-run",
            Self::Remove => "remove-run",
        }
    }

    pub fn action_entrypoint(self) -> &'static str {
        self.dialog_entrypoint().unwrap_or(self.run_entrypoint())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn run_accepts_worktrunk_base_shortcuts() {
        let cli = Cli::try_parse_from([
            "wtherdr",
            "run",
            "create",
            "--branch",
            "feature/api",
            "--base",
            "@",
        ])
        .unwrap();

        match cli.command {
            Command::Run { branch, base, .. } => {
                assert_eq!(branch.as_deref(), Some("feature/api"));
                assert_eq!(base.as_deref(), Some("@"));
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn workflows_use_manifest_entrypoint_ids() {
        assert_eq!(PopupWorkflow::Create.id(), "create");
        assert_eq!(PopupWorkflow::Create.dialog_entrypoint(), Some("create"));
        assert_eq!(PopupWorkflow::Create.run_entrypoint(), "create-run");
        assert_eq!(PopupWorkflow::Merge.run_entrypoint(), "merge-run");
        assert_eq!(PopupWorkflow::Remove.run_entrypoint(), "remove-run");
    }

    #[test]
    fn switch_opens_worktrunks_picker_without_a_dialog() {
        assert_eq!(PopupWorkflow::Switch.dialog_entrypoint(), None);
        assert_eq!(PopupWorkflow::Switch.action_entrypoint(), "switch-run");
        assert_eq!(PopupWorkflow::Merge.action_entrypoint(), "merge");
    }
}
