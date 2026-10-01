# wtherdr

A Herdr plugin that runs Worktrunk's worktree workflows in Herdr popups and keeps Herdr's nested workspaces in sync. Single portable Rust binary for macOS and Linux.

docs/architecture.md — process lifecycle, module boundaries, environment contract, reconciliation rules
docs/popups.md — popup entrypoints, dialog chrome and keys, Herdr's popup constraints
docs/repository.md — GitHub settings, labels, rulesets, and community setup

SPEC.md and specs/ hold requirements; TODO.md holds active Phases and DONE.md records shipped work.

## Structure

| Path | Purpose |
| --- | --- |
| `herdr-plugin.toml` | Actions, pane entrypoints, popup dimensions |
| `src/cli.rs` | Command surface and the workflow-to-entrypoint mapping |
| `src/dialog.rs` | Dialog layout and key handling |
| `src/terminal.rs` | Raw mode, key decoding, terminal size |
| `src/herdr.rs` | Socket client, popup requests, workspace reconciliation |
| `src/worktrunk.rs` | `wt` invocation and machine-readable results |
| `src/workflow.rs` | Workflow rules over the `Herdr` and `Worktrunk` traits |
| `scripts/install-binary.sh` | Manifest build command |

## Conventions

- Popup dimensions live in the manifest alone. Entrypoint ids in `PopupWorkflow` mirror it; run entrypoints are `<workflow>-run`.
- Herdr and Worktrunk sit behind the `Herdr` and `Worktrunk` traits so workflow rules stay provable without a live session.
- Interactive child processes inherit the terminal; capture only the machine-readable output used for reconciliation.
- Herdr API paths are absolute. Popup context travels in `WTHERDR_*` environment variables, never in argv.
- Errors name the failed operation and the workspace or path involved, and say what survived when Worktrunk succeeded but Herdr did not.

## Working here

- Write the failing test first, and keep anything renderable or parseable in a pure function so no test needs a live Herdr session.
- Use `mise run check` for the full gate. Interactive Cargo tasks live under `dev:`; `test:*` must stay safe for unattended CI and release checks.
- Commit hooks live in `prek.toml`; `mise run hooks:install` installs them. Worktrunk runs `release:check` before merging into its default branch and `check` for other targets.
- Shell tasks and tests use Bash and standard Unix utilities; developer search tools are not runtime prerequisites.
- Cargo.toml owns the version. Use `version:sync` to repair Cargo.lock and herdr-plugin.toml; reserve `version:bump` for releases.
- version:read, write, files, and verify are the Rust kit interface; version:cargo is an alias for version:read.
- Shared task regressions live in _tool/template/tests; retain local Cargo, packaging, installer, and task-selection tests.
- Never run release:bootstrap-crate or release:publish-crate as a check. CRATES_IO_PUBLISHING enables OIDC only after the first publication and trusted publisher setup.
- Release rehearsal is `mise run release:rehearse`. Pushes and publication belong to an explicitly requested release, with the task confirmation gates intact.
- Try UI changes for real with `cargo install --path . --locked --force`, then `herdr plugin link .` after manifest edits.
- Agent sandboxes block `HERDR_SOCKET_PATH`, so every `herdr` command that reaches the server fails with `PermissionDenied`. Ask the user to run those.
