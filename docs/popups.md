# Popups

## Entrypoints

| Entrypoint | Command | Size |
| --- | --- | --- |
| `create`, `merge`, `remove` | `wtherdr prompt <workflow>` | 72 × 7 cells |
| `create-run`, `switch-run`, `merge-run`, `remove-run` | `wtherdr run <workflow>` | 90% × 85% |

Dimensions live in `herdr-plugin.toml` and nowhere else: numbers are outer terminal cells, strings are a percentage of the terminal area, and anything below Herdr's popup minimum is clamped. `plugin.pane.open` can override them, but wtherdr sends geometry-free requests so the manifest stays the single place to retune a popup. Manifest changes need `herdr plugin link .` again; Rust changes need a reinstall.

## Dialog chrome

```text
╭─ 󱓊 wt create ───────────────────────────────────────────────────────╮
│
│  wtherdr  feature/api▏
│                   ↵ create     ^c clear     esc cancel
│
╰─────────────────────────────────────────────────────────────────────╯
```

- A blank row, the subject row, then a centred hint row whose confirming key is a reverse-video chip. The subject row carries the repository name dimmed, then the value being edited or confirmed.
- Herdr requires a pane title and draws it on the popup border, so the dialog renders no title of its own.
- `enter` confirms, `^c` clears the input, `esc` cancels. An escape sequence such as an arrow key is not a cancellation: `terminal::read_key` waits briefly for a following byte before deciding.
- Dialogs draw on the alternate screen and restore raw mode and the cursor on the way out, including when a workflow fails.
- A run popup that fails prints its error, then the same hint row with `close` actions, and waits for `enter` or `esc`.

`dialog::frame` is pure and takes an explicit width, so layout is tested without a terminal.

## Herdr's popup constraints

- A popup is a singleton modal. Opening one while another popup, Settings, or Copy mode is up returns `ui_busy`, which is why the dialog hands off to a detached opener instead of opening the run popup itself.
- A popup has no pane id, emits no pane lifecycle events, and does not change plugin focus context. Workspace context has to be captured before it opens.
- A popup closes when its command exits or on `popup.close`.
