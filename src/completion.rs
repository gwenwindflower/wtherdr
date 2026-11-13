use std::io::{IsTerminal, stdin};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;

use crate::dialog::{Hint, hint_row};
use crate::terminal::{self, Key, RawMode};

pub fn frame(
    action: &str,
    message: &str,
    error: Option<&str>,
    tick: Option<usize>,
    width: u16,
) -> String {
    let symbol = tick.map_or(if error.is_some() { "!" } else { "✓" }, |tick| {
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][tick % 10]
    });
    let mut lines = vec![String::new()];
    let available = usize::from(width.saturating_sub(4).max(1));
    let message = if message.is_empty() { action } else { message };
    for line in message
        .lines()
        .chain(error.into_iter().flat_map(str::lines))
    {
        let clean: String = line
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        let characters: Vec<char> = clean.chars().collect();
        for chunk in characters.chunks(available) {
            let text = if lines.len() == 1 {
                format!("{symbol} {}", chunk.iter().collect::<String>())
            } else {
                chunk.iter().collect::<String>()
            };
            lines.push(if tick.is_some() {
                format!("  \x1b[2m{text}\x1b[0m")
            } else {
                format!("  {text}")
            });
        }
    }
    let mut hints = vec![
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
    if lines.len() > 2 {
        hints.push(Hint {
            key: "↑/↓",
            label: "scroll",
            primary: false,
        });
    }
    lines.push(if tick.is_some() {
        "  \x1b[2mFinishing workspace cleanup…\x1b[0m".into()
    } else {
        hint_row(&hints, width)
    });
    lines.join("\r\n")
}

pub fn show(
    action: &str,
    message: &str,
    error: &mut Option<String>,
    cleanup: impl FnOnce() -> Result<()> + Send,
) -> Result<()> {
    if !stdin().is_terminal() {
        if let Err(failure) = cleanup() {
            append_error(error, &format!("{failure:#}"));
        }
        println!("{message}");
        if let Some(error) = error {
            eprintln!("{error}");
        }
        return Ok(());
    }
    let _screen = Screen::open()?;
    std::thread::scope(|scope| -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        scope.spawn(move || {
            let _ = sender.send(cleanup());
        });
        let mut tick = 0;
        loop {
            let content = frame(
                action,
                message,
                error.as_deref(),
                Some(tick),
                terminal::width(),
            );
            terminal::paint(&viewport(&content, 0, terminal::size().1).0)?;
            match receiver.recv_timeout(Duration::from_millis(80)) {
                Ok(result) => {
                    if let Err(failure) = result {
                        append_error(error, &format!("{failure:#}"));
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => tick += 1,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    append_error(
                        error,
                        "Workspace cleanup stopped before reporting its result",
                    );
                    break;
                }
            }
        }
        Ok(())
    })?;
    let mut offset = 0usize;
    loop {
        let content = frame(action, message, error.as_deref(), None, terminal::width());
        let (visible, max_offset) = viewport(&content, offset, terminal::size().1);
        offset = offset.min(max_offset);
        terminal::paint(&visible)?;
        match terminal::read_key()? {
            Key::Enter | Key::Escape | Key::Clear => return Ok(()),
            Key::Up => offset = offset.saturating_sub(1),
            Key::Down => offset = offset.saturating_add(1).min(max_offset),
            _ => {}
        }
    }
}

fn viewport(content: &str, offset: usize, height: u16) -> (String, usize) {
    let lines: Vec<&str> = content.split("\r\n").collect();
    let body = &lines[..lines.len().saturating_sub(1)];
    let rows = usize::from(height.saturating_sub(1));
    let max_offset = body.len().saturating_sub(rows);
    let mut visible: Vec<&str> = body
        .iter()
        .skip(offset.min(max_offset))
        .take(rows)
        .copied()
        .collect();
    if height > 0 {
        visible.push(lines.last().copied().unwrap_or_default());
    }
    (visible.join("\r\n"), max_offset)
}

fn append_error(error: &mut Option<String>, detail: &str) {
    if let Some(error) = error {
        error.push('\n');
        error.push_str(detail);
    } else {
        *error = Some(detail.to_owned());
    }
}

struct Screen {
    _raw: RawMode,
}

impl Screen {
    fn open() -> Result<Self> {
        let raw = RawMode::enable()?;
        terminal::enter_dialog_screen()?;
        Ok(Self { _raw: raw })
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = terminal::leave_dialog_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_message_does_not_claim_cleanup_has_finished() {
        let frame = frame("remove", "Removed feature", None, Some(0), 72);
        assert!(frame.contains("Finishing workspace cleanup"));
        assert!(!frame.contains("↵ close"));
    }

    #[test]
    fn completed_action_stays_visible_until_dismissed() {
        let frame = frame("rebase", "Rebased feature onto main", None, None, 72);
        assert!(frame.contains("Rebased feature onto main"));
        assert!(frame.contains("↵ close"));
        assert!(frame.contains("esc close"));
    }

    #[test]
    fn one_line_result_uses_the_create_dialogs_three_content_rows() {
        let frame = frame("remove", "Removed feature", None, None, 70);
        assert_eq!(frame.lines().count(), 3);
        assert!(frame.lines().nth(1).unwrap().contains("✓ Removed feature"));
        assert!(!frame.contains("scroll"));
    }

    #[test]
    fn failure_keeps_the_operation_and_recovery_detail() {
        let frame = frame(
            "rebase",
            "Rebase did not finish",
            Some("Resolve conflicts, then git rebase --continue"),
            None,
            72,
        );
        assert!(frame.contains("Rebase did not finish"));
        assert!(frame.contains("git rebase --continue"));
    }

    #[test]
    fn long_diagnostics_scroll_without_overflowing_the_popup() {
        let text = (0..30)
            .map(|row| format!("line {row}"))
            .collect::<Vec<_>>()
            .join("\r\n")
            + "\r\nclose";
        let (first, last_offset) = viewport(&text, 0, 8);
        assert_eq!(first.lines().count(), 8);
        assert!(first.contains("line 0"));
        assert!(!first.contains("line 7"));
        assert!(first.ends_with("close"));
        let (last, _) = viewport(&text, usize::MAX, 8);
        assert!(last.contains("line 29"));
        assert_eq!(last_offset, 23);
    }
}
