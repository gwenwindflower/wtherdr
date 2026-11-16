# Architecture

## Workflow lifecycle

1. **Action** — `wtherdr popup <workflow>`, headless. Reads `HERDR_PLUGIN_CONTEXT_JSON`, captures the source workspace id and its root directory, and opens the workflow's first popup. Focus can move while a popup is up, so this context is captured before anything opens.
2. **Dialog popup** — `wtherdr prompt <workflow>`. Names a branch or confirms the checkout, spawns the handoff opener, and exits so its popup closes. The create dialog keeps the source workspace focused by default; Tab toggles the focus option.
3. **Handoff** — `wtherdr handoff <workflow>`, detached into its own process group with no terminal. Retries `plugin.pane.open` past `ui_busy` until the dialog popup is gone, then opens the run popup carrying the dialog's input.
4. **Run popup** — `wtherdr run <workflow>`. Runs `wt`, reads its machine result, and reconciles focus and opened workspaces. Successful creation exits without a result popup; other outcomes pass to a detached completion opener before exiting.
5. **Completion popup** — `wtherdr finish`. The opener retries past the run popup's modal, then opens `complete` with the outcome and destination workspace context. The popup animates while removed workspaces close, then presents success or error details until Enter or Esc.

Switch skips steps 2 and 3: Worktrunk owns the picker, so its action opens `switch-run` directly.

Rebase and hook actions use the same filterable selection dialog. Rebase lists Git's local and remote branches; hooks come from `wt hook show --format=json`. Execution uses `wt step rebase` and `wt hook <type> --foreground <source>:<name>` respectively.

## Module boundaries

- `workflow::Engine` holds every rule about what Herdr should look like after a Worktrunk command, expressed over the `Herdr` and `Worktrunk` traits. Its tests use fakes and assert the calls a workflow makes.
- `herdr::Api` implements `Herdr` over the session socket with newline-delimited JSON. Rejections become a `RequestError` carrying Herdr's error code; `is_modal_conflict` keys the handoff retry off `ui_busy`.
- `worktrunk::CommandRunner` runs `wt -C <cwd> … --format=json` with inherited stdin and relayed stderr, so hooks, prompts, and progress stay interactive while stdout carries the result. Exit code 130 and an action of `cancelled` both mean the user backed out.
- `transcript` forwards stderr live through a terminal when the caller has one, mirrors terminal dimensions, and retains a bounded diagnostic tail for completion errors. Descendants holding stderr open do not delay a completed command.
- `selector` owns filtering, selection, and navigation for branch and hook dialogs; `completion` owns result rendering and cleanup animation.
- `invocation::source_workspace` prefers the workspace root over a focused pane's subdirectory.

## Environment contract

| Variable | Written by | Read by |
| --- | --- | --- |
| `WTHERDR_SOURCE_WORKSPACE_ID`, `WTHERDR_SOURCE_CWD` | every popup request | the dialog, handoff, and run processes |
| `WTHERDR_BRANCH`, `WTHERDR_BASE`, `WTHERDR_FOCUS` | create input, forwarded by the handoff | `wtherdr run create` |
| `WTHERDR_BASE` | rebase selection, forwarded by the handoff | `wtherdr run rebase` |
| `WTHERDR_HOOK` | hook selection, forwarded by the handoff | `wtherdr run hook` |
| `WTHERDR_OUTCOME`, `WTHERDR_WORKFLOW` | run popup, forwarded by the completion handoff | `wtherdr finish` |

`wtherdr run create` accepts `--branch`, `--base`, and `--focus`, which override the environment for manual runs outside a popup. `--focus` enables focus; `--focus=false` disables it. `WTHERDR_FOCUS` accepts `true` or `false` and defaults to `false`.

If a completion popup cannot open, the handoff attempts workspace cleanup, saves the result in a private temporary file, and requests a Herdr notification containing its path. `wtherdr finish --result <path>` reads the saved result and retries any remaining cleanup.

## Reconciliation rules

- Switching to the repository root focuses the parent workspace. Any other checkout opens as a workspace nested under the parent and labelled with its branch.
- Create opens the nested workspace without moving focus unless the focus option is enabled. The switch picker always focuses the selected checkout.
- The switch result popup belongs to the selected workspace, including when switching from the repository parent to a nested checkout. Selecting the repository root keeps the result on the parent. Worktrunk navigates between checkouts without changing their branches.
- Merge focuses the repository parent and closes the child workspace only when Worktrunk reports that it removed the checkout.
- Remove focuses the repository parent and defers closing the child workspace to the completion popup. Closing the run popup's owning workspace would terminate its process.
- Cleanup explicitly focuses the result popup's workspace after closing removed workspaces. Merge, remove, and clean use the repository parent as that destination.
- Clean confirms removal of integrated worktrees, then reads `wt list` with JSON schema 2. It selects linked checkouts, including detached checkouts, with a recognized integration reason, no staged, modified, untracked, renamed, deleted, or conflicted files, and no lock, prune marker, or operation in progress. Unknown status is ineligible. The repository root is always excluded.
- Clean runs each candidate through ordinary foreground `wt remove`, retaining hooks and approvals without force flags. It continues after individual failures and uses Git snapshots to close only registered workspaces whose candidate checkouts are confirmed removed. Workspace paths are matched before removal so symlink aliases remain identifiable. The result reports the removal count and any failures. Worktrunk must support list JSON schema 2.
- The native switch picker owns its internal removal interaction. After it exits, wtherdr compares Git worktree snapshots and closes only registered workspaces proven removed during the picker, including when the picker was cancelled or failed.
- Merge and remove refuse to run from the repository parent, in the dialog as well as the run popup.
- A Worktrunk command that succeeds while Herdr reconciliation fails reports both: what Worktrunk did, and what Herdr could not do.
