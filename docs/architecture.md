# Architecture

## A workflow crosses four processes

1. **Action** — `wtherdr popup <workflow>`, headless. Reads `HERDR_PLUGIN_CONTEXT_JSON`, captures the source workspace id and its root directory, and opens the workflow's first popup. Focus can move while a popup is up, so this context is captured before anything opens.
2. **Dialog popup** — `wtherdr prompt <workflow>`. Names a branch or confirms the checkout, spawns the handoff opener, and exits so its popup closes.
3. **Handoff** — `wtherdr handoff <workflow>`, detached into its own process group with no terminal. Retries `plugin.pane.open` past `ui_busy` until the dialog popup is gone, then opens the run popup carrying the dialog's input.
4. **Run popup** — `wtherdr run <workflow>`. Runs `wt`, reads its machine result, reconciles Herdr, and exits. Success closes the popup; failure prints the error and waits for a dismiss key.

Switch skips steps 2 and 3: Worktrunk owns the picker, so its action opens `switch-run` directly.

## Module boundaries

- `workflow::Engine` holds every rule about what Herdr should look like after a Worktrunk command, expressed over the `Herdr` and `Worktrunk` traits. Its tests use fakes and assert the calls a workflow makes.
- `herdr::Api` implements `Herdr` over the session socket with newline-delimited JSON. Rejections become a `RequestError` carrying Herdr's error code; `is_modal_conflict` keys the handoff retry off `ui_busy`.
- `worktrunk::CommandRunner` runs `wt -C <cwd> … --format=json` with stdin and stderr inherited, so hooks, prompts, and progress stay interactive while stdout carries the result. Exit code 130 and an action of `cancelled` both mean the user backed out.
- `invocation::source_workspace` prefers the workspace root over a focused pane's subdirectory.

## Environment contract

| Variable | Written by | Read by |
| --- | --- | --- |
| `WTHERDR_SOURCE_WORKSPACE_ID`, `WTHERDR_SOURCE_CWD` | every popup request | the dialog, handoff, and run processes |
| `WTHERDR_BRANCH`, `WTHERDR_BASE` | the create dialog, forwarded by the handoff | `wtherdr run create` |

`wtherdr run create` also accepts `--branch` and `--base`, which override the environment for manual runs outside a popup.

## Reconciliation rules

- Switching to the repository root focuses the parent workspace. Any other checkout opens as a workspace nested under the parent and labelled with its branch.
- Merge closes the workspace only when Worktrunk reports that it removed the checkout.
- Remove closes the workspace, then focuses the parent.
- Merge and remove refuse to run from the repository parent, in the dialog as well as the run popup.
- A Worktrunk command that succeeds while Herdr reconciliation fails reports both: what Worktrunk did, and what Herdr could not do.
