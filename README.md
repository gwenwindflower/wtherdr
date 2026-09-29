# wtherdr

[![CI](https://github.com/gwenwindflower/wtherdr/actions/workflows/ci.yml/badge.svg)](https://github.com/gwenwindflower/wtherdr/actions/workflows/ci.yml)

wtherdr is fast, light [worktrunk](https://worktrunk.dev)-[herdr](https://herdr.dev) integration that is actually good. It aims to bring the wonderful worktrunk UX into herdr as minimally and correctly as possible, without messy shell spaghetti or re-inventing pane interfaces.

- Worktrunk creates, switches, merges, and removes Git worktrees with its native hooks and interactive output, while Herdr provides an integrated interface
- The excellent `wt switch` interface is the real interface, in a Herdr popup, with the same keybindings and behavior as Worktrunk itself
- Herdr nests new or switched-to wt worktrees under the root workspace, just like a native worktree, because it is one
- Merge or remove a worktree and Herdr closes its workspace and focuses the parent.
- Rust-based using latest Herdr plugin API, with attention to details like CLI command design and install process

wtherdr supports macOS and Linux.

## Install

You need [Herdr](https://herdr.dev/docs/install/) 0.7.5 or later, [Worktrunk](https://worktrunk.dev/worktrunk/#install), Git, and a compatible [Rust toolchain with Cargo](https://www.rust-lang.org/tools/install). Cargo's binary directory—`$CARGO_HOME/bin`, normally `~/.cargo/bin`—must be on `PATH`. [Cargo Binstall](https://github.com/cargo-bins/cargo-binstall#installation) is optional, but makes installation faster when a release artifact is available for your platform.

```bash
herdr plugin install gwenwindflower/wtherdr
```

Herdr previews the plugin commands before installation. The installer reuses a matching `wtherdr` from `PATH`, then tries Cargo Binstall, then installs from source with Cargo.

Verify both integrations:

```bash
herdr plugin list --plugin wtherdr
wtherdr status
```

## Actions

| Action | Behavior |
| --- | --- |
| `wtherdr.create` | Create a branch and worktree with Worktrunk, run its hooks, and open the nested Herdr workspace. |
| `wtherdr.switch` | Show Worktrunk's native picker and focus or open the selected workspace. |
| `wtherdr.merge` | Merge the current linked worktree and close its workspace when Worktrunk removes it. |
| `wtherdr.remove` | Remove the current linked worktree, close its workspace, and focus its parent. |

Create, merge, and remove open a dialog first, where `enter` runs the workflow, `^c` clears the branch name, and `esc` cancels. Switch goes straight to Worktrunk's picker. The popup that runs Worktrunk closes itself when the command succeeds and waits for `enter` or `esc` when it fails, so the error stays readable.

Invoke an action directly or map the action through Herdr's key configuration:

```bash
herdr plugin action invoke wtherdr.switch
```

### Recommended keybindings

Keep the primary Worktrunk flow on `g` after Herdr's prefix:

| Key | Action |
| --- | --- |
| `prefix+g` | Switch worktree |
| `prefix+ctrl+g` | Merge worktree |
| `prefix+alt+g` | Remove worktree |
| `prefix+shift+g` | Create worktree |

Add these bindings to `~/.config/herdr/config.toml`. The empty built-in bindings release Herdr's default `goto` and `new_worktree` keys for wtherdr.

```toml
[keys]
goto = ""
new_worktree = ""

[[keys.command]]
key = "prefix+g"
type = "plugin_action"
command = "wtherdr.switch"
description = "󱓎 wt switch"

[[keys.command]]
key = "prefix+ctrl+g"
type = "plugin_action"
command = "wtherdr.merge"
description = " wt merge"

[[keys.command]]
key = "prefix+alt+g"
type = "plugin_action"
command = "wtherdr.remove"
description = "󱓌 wt remove"

[[keys.command]]
key = "prefix+shift+g"
type = "plugin_action"
command = "wtherdr.create"
description = "󱓊 wt create"
```

Apply the bindings to the running session:

```bash
herdr server reload-config
```

Worktrunk remains the source of truth for worktree paths, hook approval, merge strategy, and project configuration. wtherdr reads Worktrunk's machine result after the interactive command completes, then reconciles Herdr's workspaces. Cancelling a dialog or the picker leaves Herdr unchanged, and a failed merge or remove keeps its workspace.

## Install the binary yourself

Install the latest release artifact with Cargo Binstall:

```bash
cargo binstall wtherdr --git https://github.com/gwenwindflower/wtherdr
```

Or compile and install it from source:

```bash
cargo install --git https://github.com/gwenwindflower/wtherdr --locked
```

`herdr plugin install gwenwindflower/wtherdr` still registers the plugin manifest and reuses a matching binary from your `PATH`.

## Local development

Development runs on [mise](https://mise.jdx.dev), which owns the linters, release tooling, and task list. Rust comes from rustup on your `PATH`; `rust-toolchain.toml` selects stable with clippy and rustfmt.

```bash
mise trust
mise install
mise run dev:reload
```

`dev:reload` installs the checkout's binary on `PATH`, then links the manifest into Herdr. Rerun it after Rust or `herdr-plugin.toml` changes. `mise run dev` runs the full local gate first, and `mise run check` runs the same checks as CI without installing or linking anything.

`mise tasks` lists every task and its description. `dev:local` replaces a published install with the checkout; `dev:released` restores the published plugin for testing the user-facing installation path.

`AGENTS.md` indexes the design docs: `docs/architecture.md` covers the process lifecycle and module boundaries, and `docs/popups.md` covers the popup entrypoints, dialog chrome, and Herdr's popup constraints.

### Pretty tasks

There are local interactive development versions of the build and test tasks that use [cargo-pretty](https://github.com/romancitodev/cargo-pretty) for rich output. You'll need to install cargo pretty for them to work (`cargo binstall cargo-pretty-build`).

```bash
mise run dev:build
mise run b
mise run dev:test
mise run t
```

### Checks and dependency maintenance

`mise run hooks:install` installs the commit hooks. `mise run check` runs the hook sweep, pedantic Clippy, version checks, Rust and shell tests, an optimized build, and packaged-crate verification. CI runs the same lint and test task groups.

`mise run deps:check` previews lockfile and dependency requirement updates; `mise run deps:update` applies compatible lockfile updates; `mise run deps:audit` checks for known vulnerabilities and yanked crates. Upgrade previews need `cargo-edit`, and audits need `cargo-audit`:

```bash
cargo install cargo-edit --locked
cargo install cargo-audit --locked
```

These optional tools and Cargo Binstall are installed separately from the CI toolchain. Dependency maintenance runs explicitly and is outside the release gate.

## Releases

`Cargo.toml` is the version source of truth. The release tasks synchronize `Cargo.lock` and `herdr-plugin.toml`, validate the repository, and derive GitHub Release notes from conventional commits.

Releases run from a clean local `main` with `gh` authenticated. `mise run release:rehearse` runs every read-only release step and prints the notes that would ship. `mise run release` prepares the version commit, runs the full gate, pushes `main`, and publishes the release; pushing and publishing each require confirmation.

GitHub creates the release tag at the default-branch head. The `Release build` workflow verifies the tag against Cargo, builds each supported target, and attaches archives and checksums. `mise run release:verify` inspects the published result.

## License

Copyright (C) 2026 Gwyneth Windflower.

Licensed under the GNU General Public License, version 3 or any later version (`GPL-3.0-or-later`). See [LICENSE](./LICENSE).
