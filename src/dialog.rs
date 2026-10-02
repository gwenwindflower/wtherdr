use std::io::{IsTerminal, Write, stdin, stdout};

use anyhow::{Context, Result};

use crate::terminal::{self};
use crate::terminal::{Key, RawMode, enter_dialog_screen, leave_dialog_screen, paint, read_key};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const HIGHLIGHT: &str = "\x1b[7m";

pub struct Hint<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub primary: bool,
}

pub struct Dialog<'a> {
    pub context: &'a str,
    pub value: &'a str,
    pub editing: bool,
    pub hints: &'a [Hint<'a>],
}

pub fn frame(dialog: &Dialog, width: u16) -> String {
    let cursor = if dialog.editing {
        format!("{HIGHLIGHT} {RESET}")
    } else {
        String::new()
    };
    let lines = [
        String::new(),
        format!(
            "  {DIM}{}{RESET}  {BOLD}{}{RESET}{cursor}",
            dialog.context, dialog.value
        ),
        hint_row(dialog.hints, width),
    ];
    let mut rendered = lines.join("\r\n");
    rendered.push_str("\r\n");
    rendered
}

fn hint_row(hints: &[Hint], width: u16) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = hints
        .iter()
        .map(|hint| {
            if hint.primary {
                format!("{HIGHLIGHT} {} {} {RESET}", hint.key, hint.label)
            } else {
                format!("{DIM}{} {}{RESET}", hint.key, hint.label)
            }
        })
        .collect();
    let plain: usize = hints
        .iter()
        .map(|hint| hint.key.chars().count() + hint.label.chars().count() + 1)
        .sum::<usize>()
        + hints.iter().filter(|hint| hint.primary).count() * 2
        + hints.len().saturating_sub(1) * 4;
    let indent = (usize::from(width).saturating_sub(plain)) / 2;
    format!("{}{}", " ".repeat(indent), rendered.join("    "))
}

#[derive(Default)]
pub struct CreateInput {
    pub branch: String,
    pub focus: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum InputResult {
    Editing,
    Submitted,
    Cancelled,
}

impl CreateInput {
    fn edit(&mut self, key: Key) -> InputResult {
        match key {
            Key::Enter if !self.branch.trim().is_empty() => {
                self.branch = self.branch.trim().to_owned();
                return InputResult::Submitted;
            }
            Key::Escape => return InputResult::Cancelled,
            Key::Clear => self.branch.clear(),
            Key::Tab => self.focus = !self.focus,
            Key::Backspace => {
                self.branch.pop();
            }
            Key::Char(character) => self.branch.push(character),
            Key::Enter | Key::Other => {}
        }
        InputResult::Editing
    }
}

pub fn create(context: &str) -> Result<Option<CreateInput>> {
    if !stdin().is_terminal() {
        return Ok(read_plain_line("create")?.map(|branch| CreateInput {
            branch,
            focus: false,
        }));
    }
    let mut hints = [
        Hint {
            key: "↵",
            label: "create",
            primary: true,
        },
        Hint {
            key: "tab",
            label: "[ ] focus",
            primary: false,
        },
        Hint {
            key: "^c",
            label: "clear",
            primary: false,
        },
        Hint {
            key: "esc",
            label: "cancel",
            primary: false,
        },
    ];

    let mut input = CreateInput::default();
    let session = DialogScreen::open()?;
    loop {
        hints[1].label = if input.focus {
            "[x] focus"
        } else {
            "[ ] focus"
        };
        session.draw(&Dialog {
            context,
            value: &input.branch,
            editing: true,
            hints: &hints,
        })?;
        match input.edit(read_key()?) {
            InputResult::Submitted => return Ok(Some(input)),
            InputResult::Cancelled => return Ok(None),
            InputResult::Editing => {}
        }
    }
}

pub fn confirm(context: &str, subject: &str, action: &str) -> Result<bool> {
    let hints = [
        Hint {
            key: "↵",
            label: action,
            primary: true,
        },
        Hint {
            key: "esc",
            label: "cancel",
            primary: false,
        },
    ];

    let session = DialogScreen::open()?;
    session.draw(&Dialog {
        context,
        value: subject,
        editing: false,
        hints: &hints,
    })?;
    loop {
        match read_key()? {
            Key::Enter => return Ok(true),
            Key::Escape | Key::Clear => return Ok(false),
            _ => {}
        }
    }
}

pub fn wait_for_dismiss() -> Result<()> {
    if !stdin().is_terminal() {
        return Ok(());
    }
    let hints = [
        Hint {
            key: "↵",
            label: "close",
            primary: true,
        },
        Hint {
            key: "esc",
            label: "close",
            primary: false,
        },
    ];
    let _raw = RawMode::enable()?;
    let mut out = stdout();
    write!(out, "\r\n{}\r\n", hint_row(&hints, terminal::width()))?;
    out.flush()?;
    loop {
        match read_key()? {
            Key::Enter | Key::Escape | Key::Clear => return Ok(()),
            _ => {}
        }
    }
}

fn read_plain_line(action: &str) -> Result<Option<String>> {
    print!("{action}: ");
    stdout().flush()?;
    let mut line = String::new();
    stdin()
        .read_line(&mut line)
        .context("could not read the requested name")?;
    let line = line.trim().to_owned();
    Ok((!line.is_empty()).then_some(line))
}

struct DialogScreen {
    _raw: RawMode,
}

impl DialogScreen {
    fn open() -> Result<Self> {
        let raw = RawMode::enable()?;
        enter_dialog_screen()?;
        Ok(Self { _raw: raw })
    }

    #[allow(clippy::unused_self)]
    fn draw(&self, dialog: &Dialog) -> Result<()> {
        paint(&frame(dialog, terminal::width()))
    }
}

impl Drop for DialogScreen {
    fn drop(&mut self) {
        let _ = leave_dialog_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_keeps_focus_unless_tab_enables_it() {
        let mut input = CreateInput::default();
        for key in [Key::Char('a'), Key::Char('b'), Key::Backspace] {
            assert_eq!(input.edit(key), InputResult::Editing);
        }
        assert!(!input.focus);
        assert_eq!(input.edit(Key::Tab), InputResult::Editing);
        assert!(input.focus);
        assert_eq!(input.branch, "a");
        assert_eq!(input.edit(Key::Enter), InputResult::Submitted);
        assert_eq!(input.edit(Key::Tab), InputResult::Editing);
        assert!(!input.focus);
        assert_eq!(input.edit(Key::Escape), InputResult::Cancelled);
    }

    #[test]
    fn create_cannot_submit_an_empty_branch() {
        let mut input = CreateInput::default();
        assert_eq!(input.edit(Key::Enter), InputResult::Editing);
        input.edit(Key::Char('a'));
        input.edit(Key::Clear);
        assert_eq!(input.edit(Key::Enter), InputResult::Editing);
    }

    fn plain(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('\x1b') {
            out.push_str(&rest[..start]);
            let tail = &rest[start..];
            let end = tail.find('m').map_or(tail.len(), |index| index + 1);
            rest = &tail[end..];
        }
        out.push_str(rest);
        out
    }

    fn hints() -> [Hint<'static>; 3] {
        [
            Hint {
                key: "↵",
                label: "create",
                primary: true,
            },
            Hint {
                key: "^c",
                label: "clear",
                primary: false,
            },
            Hint {
                key: "esc",
                label: "cancel",
                primary: false,
            },
        ]
    }

    #[test]
    fn the_dialog_leads_with_the_repository_and_the_edited_value() {
        let hints = hints();
        let rendered = frame(
            &Dialog {
                context: "wtherdr",
                value: "feature/api",
                editing: true,
                hints: &hints,
            },
            72,
        );
        let lines: Vec<String> = plain(&rendered).lines().map(str::to_owned).collect();

        assert!(lines[0].is_empty());
        assert_eq!(lines[1], "  wtherdr  feature/api ");
        assert!(lines[2].contains("↵ create"));
    }

    #[test]
    fn hints_are_centred_and_lead_with_the_confirming_key() {
        let hints = hints();
        let row = plain(&hint_row(&hints, 72));

        let confirm = row.find("↵ create").expect("confirming key");
        let clear = row.find("^c clear").expect("clear key");
        let cancel = row.find("esc cancel").expect("cancel key");
        assert!(confirm < clear && clear < cancel);

        let leading = row
            .chars()
            .take_while(|character| *character == ' ')
            .count();
        let trailing = 72 - row.chars().count();
        assert!(leading.abs_diff(trailing) <= 2, "hint row is not centred");
    }
}
