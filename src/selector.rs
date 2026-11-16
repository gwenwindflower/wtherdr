use std::io::{IsTerminal, Write, stdin, stdout};

use anyhow::{Context, Result};

use crate::terminal::{self, Key, RawMode};

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const SELECTED: &str = "\x1b[7m";

struct Selection<'a> {
    options: &'a [String],
    default: Option<&'a str>,
    query: String,
    selected: Option<usize>,
}

#[derive(Debug, Eq, PartialEq)]
enum Edit {
    Continue,
    Submit(String),
    Cancel,
}

impl<'a> Selection<'a> {
    fn new(options: &'a [String], default: Option<&'a str>) -> Self {
        let mut selection = Self {
            options,
            default,
            query: String::new(),
            selected: None,
        };
        selection.reset_selection();
        selection
    }

    fn matches(&self) -> Vec<&str> {
        let query = self.query.trim().to_lowercase();
        self.options
            .iter()
            .filter(|option| option.to_lowercase().contains(&query))
            .map(String::as_str)
            .collect()
    }

    fn reset_selection(&mut self) {
        self.selected = if self.query.trim().is_empty() {
            self.default.map_or_else(
                || (!self.options.is_empty()).then_some(0),
                |default| self.options.iter().position(|option| option == default),
            )
        } else {
            (!self.matches().is_empty()).then_some(0)
        };
    }

    fn edit(&mut self, key: Key) -> Edit {
        match key {
            Key::Enter => {
                if let Some(value) = self
                    .selected
                    .and_then(|index| self.matches().get(index).copied())
                {
                    return Edit::Submit(value.to_owned());
                }
            }
            Key::Escape => return Edit::Cancel,
            Key::Down => {
                let length = self.matches().len();
                self.selected = (length != 0).then(|| {
                    self.selected
                        .map_or(0, |index| index.saturating_add(1).min(length - 1))
                });
            }
            Key::Up => {
                self.selected = (!self.matches().is_empty())
                    .then(|| self.selected.unwrap_or(0).saturating_sub(1));
            }
            Key::Char(character) => {
                self.query.push(character);
                self.reset_selection();
            }
            Key::Backspace => {
                self.query.pop();
                self.reset_selection();
            }
            Key::Clear => {
                self.query.clear();
                self.reset_selection();
            }
            Key::Tab | Key::Other => {}
        }
        Edit::Continue
    }

    fn frame(&self, context: &str, action: &str, width: u16, height: u16) -> String {
        let width = usize::from(width);
        let rows = usize::from(height);
        let matches = self.matches();
        let visible = rows.saturating_sub(6).max(1);
        let first = self
            .selected
            .unwrap_or(0)
            .saturating_sub(visible.saturating_sub(1));
        let selection_hint = if self.selected.is_none() && self.query.trim().is_empty() {
            self.default
                .map(|default| format!("  {default} unavailable; select a target"))
        } else {
            None
        };
        let mut lines = vec![
            String::new(),
            format!("{DIM}{}{RESET}", clipped(&format!("  {context}"), width)),
            clipped(&format!("  > {}", self.query), width),
            selection_hint.map_or_else(String::new, |hint| {
                format!("{DIM}{}{RESET}", clipped(&hint, width))
            }),
        ];
        if matches.is_empty() {
            let empty_message = if self.options.is_empty() {
                if action == "hook" {
                    "  No configured hooks"
                } else {
                    "  No available options"
                }
            } else {
                "  No matching options"
            };
            lines.push(format!("{DIM}{}{RESET}", clipped(empty_message, width)));
        } else {
            for (index, value) in matches.iter().enumerate().skip(first).take(visible) {
                let value = clipped(&format!("  {value}"), width);
                if Some(index) == self.selected {
                    lines.push(format!("{SELECTED}{value}{RESET}"));
                } else {
                    lines.push(value);
                }
            }
        }
        let full_hint = format!("  ↵ {action}    ^j/^k ^n/^p navigate    ^c clear    esc cancel");
        let short_hint = format!("  ↵ {action}  ↑↓ move  esc cancel");
        let hints = if full_hint.chars().count() <= width {
            full_hint
        } else if short_hint.chars().count() <= width {
            short_hint
        } else {
            "↵ ok esc x".into()
        };
        lines.push(String::new());
        lines.push(format!("{DIM}{}{RESET}", clipped(&hints, width)));
        lines.truncate(rows);
        lines.join("\r\n")
    }
}

fn clipped(text: &str, width: usize) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(width)
        .collect()
}

pub fn choose(
    context: &str,
    action: &str,
    options: &[String],
    default: Option<&str>,
) -> Result<Option<String>> {
    let mut selection = Selection::new(options, default);
    if !stdin().is_terminal() {
        print!("{action}: ");
        stdout().flush()?;
        let mut line = String::new();
        if stdin()
            .read_line(&mut line)
            .context("could not read the selection")?
            == 0
        {
            return Ok(None);
        }
        for character in line.trim().chars() {
            selection.edit(Key::Char(character));
        }
        return Ok(match selection.edit(Key::Enter) {
            Edit::Submit(value) => Some(value),
            Edit::Continue | Edit::Cancel => None,
        });
    }
    let _screen = SelectionScreen::open()?;
    loop {
        let (width, height) = terminal::size();
        terminal::paint(&selection.frame(context, action, width, height))?;
        match selection.edit(terminal::read_key()?) {
            Edit::Submit(value) => return Ok(Some(value)),
            Edit::Cancel => return Ok(None),
            Edit::Continue => {}
        }
    }
}

struct SelectionScreen {
    _raw: RawMode,
}

impl SelectionScreen {
    fn open() -> Result<Self> {
        let raw = RawMode::enable()?;
        terminal::enter_dialog_screen()?;
        Ok(Self { _raw: raw })
    }
}

impl Drop for SelectionScreen {
    fn drop(&mut self) {
        let _ = terminal::leave_dialog_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Vec<String> {
        ["feature/api", "main", "release", "feature/docs"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn blank_input_submits_the_existing_default_branch() {
        let options = options();
        let mut selection = Selection::new(&options, Some("main"));
        assert_eq!(selection.edit(Key::Enter), Edit::Submit("main".into()));
    }

    #[test]
    fn navigation_overrides_the_default() {
        let options = options();
        let mut selection = Selection::new(&options, Some("main"));
        selection.edit(Key::Down);
        assert_eq!(selection.edit(Key::Enter), Edit::Submit("release".into()));
    }

    #[test]
    fn missing_default_requires_an_explicit_selection() {
        let options = vec!["feature/api".into(), "release".into()];
        let mut selection = Selection::new(&options, Some("main"));
        assert_eq!(selection.edit(Key::Enter), Edit::Continue);
        assert!(
            selection
                .frame("repo", "rebase", 72, 12)
                .contains("main unavailable")
        );
        selection.edit(Key::Down);
        assert_eq!(
            selection.edit(Key::Enter),
            Edit::Submit("feature/api".into())
        );
        selection.edit(Key::Clear);
        assert_eq!(selection.edit(Key::Enter), Edit::Continue);
        for character in "release".chars() {
            selection.edit(Key::Char(character));
        }
        assert_eq!(selection.edit(Key::Enter), Edit::Submit("release".into()));
    }

    #[test]
    fn empty_hook_list_explains_no_hooks_are_configured_and_can_be_cancelled() {
        let mut selection = Selection::new(&[], None);
        let rendered = selection.frame("repo", "hook", 72, 12);
        assert!(rendered.contains("No configured hooks"));
        assert_eq!(selection.edit(Key::Enter), Edit::Continue);
        assert_eq!(selection.edit(Key::Escape), Edit::Cancel);
    }

    #[test]
    fn filtering_and_navigation_select_only_existing_options() {
        let options = options();
        let mut selection = Selection::new(&options, Some("main"));
        for character in "FEATURE/".chars() {
            selection.edit(Key::Char(character));
        }
        selection.edit(Key::Down);
        assert_eq!(
            selection.edit(Key::Enter),
            Edit::Submit("feature/docs".into())
        );
        selection.edit(Key::Up);
        assert_eq!(
            selection.edit(Key::Enter),
            Edit::Submit("feature/api".into())
        );
    }

    #[test]
    fn unmatched_query_cannot_submit_and_clear_restores_default() {
        let options = options();
        let mut selection = Selection::new(&options, Some("main"));
        selection.edit(Key::Char('z'));
        assert_eq!(selection.edit(Key::Enter), Edit::Continue);
        selection.edit(Key::Clear);
        assert_eq!(selection.edit(Key::Enter), Edit::Submit("main".into()));
        assert_eq!(selection.edit(Key::Escape), Edit::Cancel);
    }

    #[test]
    fn backspace_recomputes_matches_and_navigation_on_empty_list_is_safe() {
        let options = options();
        let mut selection = Selection::new(&options, None);
        selection.edit(Key::Char('z'));
        selection.edit(Key::Down);
        selection.edit(Key::Up);
        selection.edit(Key::Backspace);
        assert_eq!(
            selection.edit(Key::Enter),
            Edit::Submit("feature/api".into())
        );
        let mut empty = Selection::new(&[], None);
        empty.edit(Key::Down);
        empty.edit(Key::Up);
        assert_eq!(empty.edit(Key::Enter), Edit::Continue);
    }

    #[test]
    fn selection_stays_visible_when_the_list_scrolls() {
        let options: Vec<_> = (0..20).map(|index| format!("branch-{index:02}")).collect();
        let mut selection = Selection::new(&options, None);
        for _ in 0..15 {
            selection.edit(Key::Down);
        }
        let rendered = selection.frame("repo", "rebase", 72, 10);
        assert!(rendered.contains("branch-15"));
        assert!(!rendered.contains("branch-00"));
        assert!(rendered.lines().count() <= 10);
    }

    #[test]
    fn frame_clips_text_and_hints_to_narrow_terminals() {
        let options = vec!["feature/a-very-long-branch".into()];
        let selection = Selection::new(&options, None);
        let rendered = selection.frame("a-long-repository", "rebase", 12, 8);
        for line in plain(&rendered).lines() {
            assert!(line.chars().count() <= 12, "{line:?}");
        }
        assert!(rendered.contains("esc"));
    }

    fn plain(text: &str) -> String {
        let mut output = String::new();
        let mut escaped = false;
        for character in text.chars() {
            if character == '\u{1b}' {
                escaped = true;
            } else if escaped {
                if character == 'm' {
                    escaped = false;
                }
            } else {
                output.push(character);
            }
        }
        output
    }
}
