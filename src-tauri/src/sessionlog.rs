//! Record a terminal session's output to a file, either raw (replayable with
//! `cat`, colours intact) or as plain text with escape sequences removed.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// Removes ANSI/VT escape sequences from a byte stream. Stateful, because a
/// sequence can be split across two chunks of terminal output.
#[derive(Debug, Default)]
pub struct AnsiStripper {
    state: State,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Text,
    /// Saw ESC.
    Escape,
    /// Inside `ESC [ ...`, until a final byte 0x40..=0x7E.
    Csi,
    /// Inside `ESC ] ...` (or P/X/^/_ strings), until BEL or `ESC \`.
    Osc,
    /// Saw ESC inside an OSC string; `\` ends it.
    OscEscape,
    /// `ESC (` style: exactly one more byte to skip.
    Charset,
}

impl AnsiStripper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append the printable part of `input` to `out`. Keeps newlines and
    /// tabs, drops carriage returns and other C0 controls. UTF-8 multi-byte
    /// characters pass through untouched (their bytes are all >= 0x80).
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &b in input {
            self.state = match (self.state, b) {
                (State::Text, 0x1b) => State::Escape,
                (State::Text, b'\n' | b'\t') => {
                    out.push(b);
                    State::Text
                }
                (State::Text, b) if b < 0x20 || b == 0x7f => State::Text,
                (State::Text, b) => {
                    out.push(b);
                    State::Text
                }
                (State::Escape, b'[') => State::Csi,
                (State::Escape, b']' | b'P' | b'X' | b'^' | b'_') => State::Osc,
                (State::Escape, b'(' | b')' | b'*' | b'+' | b'#' | b'%') => State::Charset,
                (State::Escape, _) => State::Text,
                (State::Csi, 0x40..=0x7e) => State::Text,
                (State::Csi, _) => State::Csi,
                (State::Osc, 0x07) => State::Text,
                (State::Osc, 0x1b) => State::OscEscape,
                (State::Osc, _) => State::Osc,
                (State::OscEscape, b'\\') => State::Text,
                (State::OscEscape, _) => State::Osc,
                (State::Charset, _) => State::Text,
            };
        }
    }
}

/// An open session log.
pub struct SessionLog {
    file: BufWriter<File>,
    stripper: Option<AnsiStripper>,
    scratch: Vec<u8>,
}

impl SessionLog {
    /// Open (append to) `path`. `plain` strips escape sequences.
    pub fn open(path: &Path, plain: bool, header: &str) -> io::Result<Self> {
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        // Logs can contain anything shown on screen: owner-only when created.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(path)?;
        let mut log = Self {
            file: BufWriter::new(file),
            stripper: plain.then(AnsiStripper::new),
            scratch: Vec::new(),
        };
        if plain && !header.is_empty() {
            writeln!(log.file, "{header}")?;
        }
        log.file.flush()?;
        Ok(log)
    }

    pub fn write(&mut self, data: &[u8]) -> io::Result<()> {
        match self.stripper.as_mut() {
            Some(s) => {
                self.scratch.clear();
                s.feed(data, &mut self.scratch);
                self.file.write_all(&self.scratch)?;
            }
            None => self.file.write_all(data)?,
        }
        // Flush every chunk so a crash or forced quit loses nothing.
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(chunks: &[&[u8]]) -> String {
        let mut s = AnsiStripper::new();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, &mut out);
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn strips_colours_titles_and_controls() {
        assert_eq!(
            strip(&[b"\x1b[1;32muser@host\x1b[0m:~$ ls\r\n"]),
            "user@host:~$ ls\n"
        );
        // OSC window title, ended by BEL and by ST.
        assert_eq!(strip(&[b"\x1b]0;my title\x07ok\x1b]2;t\x1b\\!"]), "ok!");
        // Charset designation, keypad mode, bell and backspace.
        assert_eq!(strip(&[b"\x1b(Ba\x1b=b\x07c\x08d"]), "abcd");
        // UTF-8 survives.
        assert_eq!(strip(&["héllo ✓\n".as_bytes()]), "héllo ✓\n");
    }

    #[test]
    fn sequences_split_across_chunks() {
        assert_eq!(
            strip(&[b"a\x1b", b"[3", b"1mb", b"\x1b]0;ti", b"tle\x07c"]),
            "abc"
        );
    }

    #[test]
    fn log_file_raw_and_plain() {
        let dir = tempfile::TempDir::new().unwrap();
        let raw = dir.path().join("raw.log");
        let mut l = SessionLog::open(&raw, false, "ignored for raw").unwrap();
        l.write(b"\x1b[31mred\x1b[0m\r\n").unwrap();
        assert_eq!(std::fs::read(&raw).unwrap(), b"\x1b[31mred\x1b[0m\r\n");

        let plain = dir.path().join("plain.log");
        let mut l = SessionLog::open(&plain, true, "# header").unwrap();
        l.write(b"\x1b[31mred\x1b[0m\r\n").unwrap();
        drop(l);
        // Appends on reopen instead of truncating.
        let mut l = SessionLog::open(&plain, true, "# again").unwrap();
        l.write(b"more\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&plain).unwrap(),
            "# header\nred\n# again\nmore\n"
        );
    }
}
