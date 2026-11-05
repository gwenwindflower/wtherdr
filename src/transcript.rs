use std::fs::File;
use std::io::{self, IsTerminal, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, anyhow};

const TAIL_LIMIT: usize = 64 * 1024;

pub fn run(command: &mut Command) -> Result<Output> {
    let mut stderr = io::stderr();
    run_with_writer(command, stderr.is_terminal(), &mut stderr)
}

fn run_with_writer(
    command: &mut Command,
    terminal: bool,
    forward: &mut (impl Write + Send),
) -> Result<Output> {
    let (mut reader, child_stderr) = if terminal {
        terminal_pair()?
    } else {
        let (reader, writer) = UnixStream::pair().context("could not create the stderr relay")?;
        (File::from(OwnedFd::from(reader)), OwnedFd::from(writer))
    };
    command.stderr(Stdio::from(child_stderr));
    let child_done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let recording = scope.spawn(|| {
            let mut reader = PollReader {
                file: &mut reader,
                child_done: &child_done,
                terminal,
                size: None,
                drain_remaining: TAIL_LIMIT,
            };
            relay(&mut reader, forward)
        });
        let output = command.output();
        child_done.store(true, Ordering::Release);
        command.stderr(Stdio::inherit());
        let stderr = recording
            .join()
            .map_err(|_| anyhow!("the stderr relay stopped unexpectedly"))?
            .context("could not relay the command's stderr")?;
        let mut output = output.with_context(|| {
            format!(
                "could not start {}",
                command.get_program().to_string_lossy()
            )
        })?;
        output.stderr = stderr;
        Ok(output)
    })
}

struct PollReader<'a> {
    file: &'a mut File,
    child_done: &'a AtomicBool,
    terminal: bool,
    size: Option<[u16; 4]>,
    drain_remaining: usize,
}

impl Read for PollReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.terminal {
                mirror_size(
                    io::stderr().as_raw_fd(),
                    self.file.as_raw_fd(),
                    &mut self.size,
                );
            }
            let done = self.child_done.load(Ordering::Acquire);
            if done && self.drain_remaining == 0 {
                return Ok(0);
            }
            let mut descriptor = libc::pollfd {
                fd: self.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll receives one initialized pollfd for this reader's file.
            let ready = unsafe { libc::poll(&raw mut descriptor, 1, if done { 0 } else { 50 }) };
            match ready {
                -1 => {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
                0 if done => return Ok(0),
                0 => {}
                _ => {
                    let limit = if done {
                        buffer.len().min(self.drain_remaining)
                    } else {
                        buffer.len()
                    };
                    let count = self.file.read(&mut buffer[..limit])?;
                    if done {
                        self.drain_remaining -= count;
                    }
                    return Ok(count);
                }
            }
        }
    }
}

fn mirror_size(source: RawFd, target: RawFd, previous: &mut Option<[u16; 4]>) {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::uninit();
    // SAFETY: ioctl fills the provided winsize for the source terminal.
    if unsafe { libc::ioctl(source, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
        return;
    }
    // SAFETY: ioctl succeeded, so the winsize is initialized.
    let size = unsafe { size.assume_init() };
    let values = [size.ws_row, size.ws_col, size.ws_xpixel, size.ws_ypixel];
    if *previous != Some(values) {
        // SAFETY: ioctl reads an initialized winsize and target is the relay terminal.
        if unsafe { libc::ioctl(target, libc::TIOCSWINSZ, &raw const size) } == 0 {
            *previous = Some(values);
        }
    }
}

fn terminal_pair() -> Result<(File, OwnedFd)> {
    let mut master = -1;
    let mut slave = -1;
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: ioctl fills the initialized winsize for the process stderr.
    unsafe { libc::ioctl(io::stderr().as_raw_fd(), libc::TIOCGWINSZ, &raw mut size) };
    // SAFETY: openpty fills both descriptors and reads the initialized winsize.
    if unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut size,
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("could not open a terminal for stderr");
    }
    // SAFETY: openpty succeeded and returned two owned descriptors.
    let master = unsafe { File::from_raw_fd(master) };
    // SAFETY: the slave descriptor belongs to this function until transferred to Stdio.
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    for descriptor in [master.as_raw_fd(), slave.as_raw_fd()] {
        // SAFETY: both descriptors are open and F_SETFD takes an integer flag.
        if unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error())
                .context("could not protect stderr relay descriptors");
        }
    }
    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: tcgetattr fills termios for the open slave terminal.
    if unsafe { libc::tcgetattr(slave.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error()).context("could not read the stderr terminal mode");
    }
    // SAFETY: tcgetattr succeeded, so mode is initialized.
    let mut mode = unsafe { mode.assume_init() };
    mode.c_oflag &= !libc::OPOST;
    // SAFETY: mode is initialized and the slave descriptor is open.
    if unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &raw const mode) } != 0 {
        return Err(io::Error::last_os_error()).context("could not configure the stderr terminal");
    }
    Ok((master, slave))
}

fn relay(reader: &mut impl Read, forward: &mut impl Write) -> io::Result<Vec<u8>> {
    let mut tail = Vec::with_capacity(TAIL_LIMIT);
    let mut chunk = [0u8; 8192];
    let mut forward_error = None;
    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) => return Err(error),
        };
        if forward_error.is_none() {
            forward_error = forward
                .write_all(&chunk[..count])
                .and_then(|()| forward.flush())
                .err();
        }
        let overflow = (tail.len() + count).saturating_sub(TAIL_LIMIT);
        tail.drain(..overflow);
        tail.extend_from_slice(&chunk[..count]);
    }
    forward_error.map_or(Ok(tail), Err)
}

pub fn diagnostic(bytes: &[u8]) -> String {
    let text = clean(bytes);
    let mut start = text.len().saturating_sub(16 * 1024);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if start == 0 {
        text
    } else {
        format!("[earlier output omitted]\n{}", &text[start..])
    }
}

pub fn clean(bytes: &[u8]) -> String {
    let mut plain = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if byte == 0x1b {
            let Some(next) = bytes.get(index).copied() else {
                break;
            };
            index += 1;
            match next {
                b'[' => {
                    while let Some(byte) = bytes.get(index) {
                        index += 1;
                        if (0x40..=0x7e).contains(byte) {
                            break;
                        }
                    }
                }
                b']' | b'P' | b'X' | b'^' | b'_' => {
                    while index < bytes.len() {
                        if bytes[index] == 0x07 {
                            index += 1;
                            break;
                        }
                        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
                            index += 2;
                            break;
                        }
                        index += 1;
                    }
                }
                0x20..=0x2f => {
                    while bytes
                        .get(index)
                        .is_some_and(|byte| (0x20..=0x2f).contains(byte))
                    {
                        index += 1;
                    }
                    index += usize::from(index < bytes.len());
                }
                _ => {}
            }
        } else if byte == b'\r' {
            if bytes.get(index) != Some(&b'\n') {
                plain.push(b'\n');
            }
        } else if byte == b'\n' || byte == b'\t' || (byte >= 0x20 && byte != 0x7f) {
            plain.push(byte);
        }
    }
    String::from_utf8_lossy(&plain)
        .chars()
        .filter(|character| !character.is_control() || matches!(*character, '\n' | '\t'))
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::process::Command;

    use super::*;

    #[test]
    fn relay_forwards_every_byte_and_retains_only_the_tail() {
        let bytes: Vec<u8> = (0..TAIL_LIMIT + 1024)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect();
        let mut forwarded = Vec::new();
        let retained = relay(&mut Cursor::new(&bytes), &mut forwarded).unwrap();
        assert_eq!(forwarded, bytes);
        assert_eq!(retained, bytes[bytes.len() - TAIL_LIMIT..]);
    }

    #[test]
    fn terminal_stderr_stays_a_terminal_and_stdout_and_status_survive() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "test -t 2 || exit 7; printf 'result'; printf 'specific failure\\n' >&2; exit 4",
        ]);
        let mut forwarded = Vec::new();
        let output = run_with_writer(&mut command, true, &mut forwarded).unwrap();
        assert_eq!(output.status.code(), Some(4));
        assert_eq!(output.stdout, b"result");
        assert_eq!(output.stderr, b"specific failure\n");
        assert_eq!(clean(&output.stderr), "specific failure");
        assert_eq!(forwarded, output.stderr);
    }

    #[test]
    fn unattended_stderr_is_a_pipe_and_its_diagnostic_survives() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "test ! -t 2 || exit 7; printf 'unattended failure\\n' >&2; exit 2",
        ]);
        let mut forwarded = Vec::new();
        let output = run_with_writer(&mut command, false, &mut forwarded).unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(clean(&output.stderr), "unattended failure");
        assert_eq!(forwarded, output.stderr);
    }

    #[test]
    fn configured_stdin_reaches_the_child() {
        let (mut input, child_input) = std::os::unix::net::UnixStream::pair().unwrap();
        input.write_all(b"approved\n").unwrap();
        drop(input);
        let mut command = Command::new("sh");
        command.args(["-c", "read answer; printf '%s' \"$answer\""]);
        command.stdin(std::process::Stdio::from(std::os::fd::OwnedFd::from(
            child_input,
        )));
        let output = run_with_writer(&mut command, true, &mut Vec::new()).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"approved");
    }

    #[test]
    fn child_start_failure_does_not_leave_the_relay_waiting() {
        let mut command = Command::new("/wtherdr-test-no-such-executable");
        assert!(run_with_writer(&mut command, true, &mut Vec::new()).is_err());
    }

    #[test]
    fn descendants_retaining_stderr_do_not_delay_the_completed_command() {
        for terminal in [false, true] {
            let mut command = Command::new("sh");
            command.args([
                "-c",
                "sleep 1 >/dev/null & printf 'direct child failure\\n' >&2; exit 3",
            ]);
            let start = std::time::Instant::now();
            let output = run_with_writer(&mut command, terminal, &mut Vec::new()).unwrap();
            assert!(start.elapsed() < std::time::Duration::from_millis(500));
            assert_eq!(output.status.code(), Some(3));
            assert_eq!(clean(&output.stderr), "direct child failure");
        }
    }

    #[test]
    fn terminal_resize_updates_the_relay_terminal() {
        let (source, _source_slave) = terminal_pair().unwrap();
        let (target, target_slave) = terminal_pair().unwrap();
        let mut previous = None;
        for (rows, columns) in [(30, 100), (45, 140)] {
            let size = libc::winsize {
                ws_row: rows,
                ws_col: columns,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            // SAFETY: ioctl reads the initialized size for the open source terminal.
            assert_eq!(
                unsafe { libc::ioctl(source.as_raw_fd(), libc::TIOCSWINSZ, &raw const size) },
                0
            );
            mirror_size(source.as_raw_fd(), target.as_raw_fd(), &mut previous);
            let mut actual = std::mem::MaybeUninit::<libc::winsize>::uninit();
            // SAFETY: ioctl fills the provided size for the open target terminal.
            assert_eq!(
                unsafe {
                    libc::ioctl(
                        target_slave.as_raw_fd(),
                        libc::TIOCGWINSZ,
                        actual.as_mut_ptr(),
                    )
                },
                0
            );
            // SAFETY: ioctl succeeded, so actual is initialized.
            let actual = unsafe { actual.assume_init() };
            assert_eq!((actual.ws_row, actual.ws_col), (rows, columns));
        }
    }

    #[test]
    fn diagnostics_strip_terminal_commands_and_keep_unicode_and_recovery_text() {
        let raw = b"\x1b[?1049h\x1b[31merror:\x1b[0m conflicted\r\n\x1b]8;;https://example.com\x1b\\git rebase --abort\x1b]8;;\x1b\\\r\n";
        assert_eq!(clean(raw), "error: conflicted\ngit rebase --abort");
        assert_eq!(clean("\x1b[2méchec\x1b[0m\x07".as_bytes()), "échec");
    }

    #[test]
    fn diagnostic_tail_fits_the_popup_handoff_environment() {
        let text = "échec\n".repeat(20_000) + "actual failure";
        let diagnostic = diagnostic(text.as_bytes());
        assert!(diagnostic.len() < 17 * 1024);
        assert!(diagnostic.ends_with("actual failure"));
        assert!(diagnostic.starts_with("[earlier output omitted]"));
    }
}
