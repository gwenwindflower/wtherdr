use std::io::{Read, Write, stdin, stdout};
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;

use anyhow::{Result, anyhow, bail};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Enter,
    Escape,
    Backspace,
    Clear,
    Tab,
    Up,
    Down,
    Char(char),
    Other,
}

pub fn key_from_byte(byte: u8) -> Option<Key> {
    match byte {
        b'\r' => Some(Key::Enter),
        0x0a | 0x0e => Some(Key::Down),
        0x0b | 0x10 => Some(Key::Up),
        b'\t' => Some(Key::Tab),
        0x03 | 0x15 => Some(Key::Clear),
        0x08 | 0x7f => Some(Key::Backspace),
        0x1b => None,
        byte if byte < 0x20 => Some(Key::Other),
        _ => None,
    }
}

pub struct RawMode {
    original: libc::termios,
}

impl RawMode {
    pub fn enable() -> Result<Self> {
        let fd = stdin().as_raw_fd();
        let mut original = MaybeUninit::<libc::termios>::uninit();
        // SAFETY: fd is the process stdin and the call only fills the provided termios.
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
            bail!("could not read the terminal mode");
        }
        // SAFETY: tcgetattr succeeded, so the value is initialized.
        let original = unsafe { original.assume_init() };

        let mut raw = original;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        raw.c_iflag &= !(libc::IXON | libc::ICRNL);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        // SAFETY: raw is a complete termios read from the same descriptor.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw const raw) } != 0 {
            bail!("could not switch the terminal to raw mode");
        }
        Ok(Self { original })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: original was read from this descriptor when raw mode started.
        unsafe {
            libc::tcsetattr(stdin().as_raw_fd(), libc::TCSANOW, &raw const self.original);
        }
    }
}

pub trait KeySource {
    fn next_byte(&mut self) -> Result<u8>;
    fn pending_byte(&mut self) -> Option<u8>;
}

pub struct TerminalKeys;

impl KeySource for TerminalKeys {
    fn next_byte(&mut self) -> Result<u8> {
        let mut byte = [0u8; 1];
        if stdin().read(&mut byte)? == 0 {
            bail!("the terminal closed while waiting for a key");
        }
        Ok(byte[0])
    }

    fn pending_byte(&mut self) -> Option<u8> {
        let mut poll = libc::pollfd {
            fd: stdin().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll receives one initialized pollfd for the process stdin.
        if unsafe { libc::poll(&raw mut poll, 1, 50) } <= 0 {
            return None;
        }
        self.next_byte().ok()
    }
}

pub fn read_key() -> Result<Key> {
    read_key_from(&mut TerminalKeys)
}

pub fn read_key_from(source: &mut impl KeySource) -> Result<Key> {
    let byte = source.next_byte()?;
    if let Some(key) = key_from_byte(byte) {
        return Ok(key);
    }
    if byte == 0x1b {
        return Ok(read_escape(source));
    }
    read_char(source, byte).map(Key::Char)
}

fn read_escape(source: &mut impl KeySource) -> Key {
    let Some(byte) = source.pending_byte() else {
        return Key::Escape;
    };
    if byte == b'[' || byte == b'O' {
        while let Some(byte) = source.pending_byte() {
            if byte.is_ascii_alphabetic() || byte == b'~' {
                return match byte {
                    b'A' => Key::Up,
                    b'B' => Key::Down,
                    _ => Key::Other,
                };
            }
        }
    }
    Key::Other
}

fn read_char(source: &mut impl KeySource, leader: u8) -> Result<char> {
    let extra = match leader {
        0xc0..=0xdf => 1,
        0xe0..=0xef => 2,
        0xf0..=0xf7 => 3,
        _ => 0,
    };
    let mut bytes = vec![leader];
    for _ in 0..extra {
        bytes.push(source.next_byte()?);
    }
    let text = String::from_utf8(bytes)?;
    text.chars()
        .next()
        .ok_or_else(|| anyhow!("the terminal sent an empty key"))
}

pub fn size() -> (u16, u16) {
    let mut size = MaybeUninit::<libc::winsize>::uninit();
    // SAFETY: the ioctl fills the provided winsize for the process stdout.
    let read = unsafe { libc::ioctl(stdout().as_raw_fd(), libc::TIOCGWINSZ, size.as_mut_ptr()) };
    if read != 0 {
        return (80, 24);
    }
    // SAFETY: the ioctl succeeded, so the value is initialized.
    let size = unsafe { size.assume_init() };
    (
        if size.ws_col == 0 { 80 } else { size.ws_col },
        if size.ws_row == 0 { 24 } else { size.ws_row },
    )
}

pub fn width() -> u16 {
    size().0
}

pub fn enter_dialog_screen() -> Result<()> {
    let mut out = stdout();
    write!(out, "\x1b[?1049h\x1b[?25l")?;
    out.flush()?;
    Ok(())
}

pub fn leave_dialog_screen() -> Result<()> {
    let mut out = stdout();
    write!(out, "\x1b[?25h\x1b[?1049l")?;
    out.flush()?;
    Ok(())
}

pub fn paint(frame: &str) -> Result<()> {
    let mut out = stdout();
    write!(out, "\x1b[H\x1b[2J{frame}")?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorded(std::collections::VecDeque<u8>);

    impl Recorded {
        fn new(bytes: &[u8]) -> Self {
            Self(bytes.iter().copied().collect())
        }
    }

    impl KeySource for Recorded {
        fn next_byte(&mut self) -> Result<u8> {
            self.0.pop_front().ok_or_else(|| anyhow!("no more keys"))
        }

        fn pending_byte(&mut self) -> Option<u8> {
            self.0.pop_front()
        }
    }

    #[test]
    fn control_bytes_map_to_dialog_keys() {
        assert_eq!(key_from_byte(b'\t'), Some(Key::Tab));
        assert_eq!(key_from_byte(b'\r'), Some(Key::Enter));
        assert_eq!(key_from_byte(0x03), Some(Key::Clear));
        assert_eq!(key_from_byte(0x7f), Some(Key::Backspace));
        assert_eq!(key_from_byte(b'a'), None);
        assert_eq!(key_from_byte(0x1b), None);
    }

    #[test]
    fn selector_control_keys_and_arrows_navigate_without_submitting() {
        for byte in [0x0a, 0x0e] {
            assert_eq!(
                read_key_from(&mut Recorded::new(&[byte])).unwrap(),
                Key::Down
            );
        }
        for byte in [0x0b, 0x10] {
            assert_eq!(read_key_from(&mut Recorded::new(&[byte])).unwrap(), Key::Up);
        }
        assert_eq!(
            read_key_from(&mut Recorded::new(b"\x1b[B")).unwrap(),
            Key::Down
        );
        assert_eq!(
            read_key_from(&mut Recorded::new(b"\x1b[A")).unwrap(),
            Key::Up
        );
        assert_eq!(
            read_key_from(&mut Recorded::new(b"\r")).unwrap(),
            Key::Enter
        );
    }

    #[test]
    fn a_bare_escape_cancels_and_an_escape_sequence_does_not() {
        assert_eq!(
            read_key_from(&mut Recorded::new(b"\x1b")).unwrap(),
            Key::Escape
        );
        assert_eq!(
            read_key_from(&mut Recorded::new(b"\x1b[A")).unwrap(),
            Key::Up
        );
    }

    #[test]
    fn multibyte_keys_decode_as_one_character() {
        assert_eq!(
            read_key_from(&mut Recorded::new("é".as_bytes())).unwrap(),
            Key::Char('é')
        );
    }
}
