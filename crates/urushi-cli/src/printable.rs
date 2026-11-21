//! Standard output without terminal control characters.
//!
//! Much of what the CLI prints was written by someone else: Jira summaries,
//! descriptions and comments, thread titles, pane text. Printed raw, an escape sequence
//! in any of them would act on the terminal that shows it (rewrite the screen, change
//! its title, or answer queries into the shell's input). [`stdout`] drops every
//! control character except newline and tab, C1 controls included, so text only ever
//! prints as text.

use std::io::{self, Write};

/// Standard output, locked, with control characters dropped.
pub(crate) fn stdout() -> Printable<io::StdoutLock<'static>> {
    Printable::new(io::stdout().lock())
}

/// A writer that passes text through without control characters.
pub(crate) struct Printable<W: Write> {
    inner: W,
    /// The last byte written was 0xC2, the first byte of a C1 control's UTF-8 form if
    /// the next is 0x80..=0x9F.
    pending_c2: bool,
}

impl<W: Write> Printable<W> {
    pub(crate) const fn new(inner: W) -> Self {
        Self {
            inner,
            pending_c2: false,
        }
    }
}

impl<W: Write> Write for Printable<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut kept = Vec::with_capacity(buf.len() + 1);
        for &byte in buf {
            if std::mem::take(&mut self.pending_c2) {
                if (0x80..=0x9f).contains(&byte) {
                    continue;
                }
                kept.push(0xc2);
            }
            match byte {
                b'\n' | b'\t' => kept.push(byte),
                0x00..=0x1f | 0x7f => {}
                0xc2 => self.pending_c2 = true,
                _ => kept.push(byte),
            }
        }
        self.inner.write_all(&kept)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if std::mem::take(&mut self.pending_c2) {
            self.inner.write_all(&[0xc2])?;
        }
        self.inner.flush()
    }
}

impl<W: Write> Drop for Printable<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(text: &str) -> String {
        let mut out = Vec::new();
        {
            let mut p = Printable::new(&mut out);
            write!(p, "{text}").unwrap();
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn text_prints_as_text() {
        assert_eq!(
            printed("UR-57 Security audit\n\tdone ✓ é ¢"),
            "UR-57 Security audit\n\tdone ✓ é ¢"
        );
    }

    #[test]
    fn escape_sequences_and_other_controls_are_dropped() {
        assert_eq!(
            printed("a\x1b]0;owned\x07b\x1b[2Jc\r\x08d\x7f\u{9b}31me"),
            "a]0;ownedb[2Jcd31me"
        );
    }

    #[test]
    fn a_c1_control_split_across_writes_is_dropped_too() {
        let mut out = Vec::new();
        {
            let mut p = Printable::new(&mut out);
            p.write_all(b"x\xc2").unwrap();
            p.write_all(b"\x9by \xc2").unwrap();
            p.write_all(b"\xa2").unwrap();
        }
        assert_eq!(String::from_utf8(out).unwrap(), "xy ¢");
    }
}
