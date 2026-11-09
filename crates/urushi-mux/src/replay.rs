//! Saving a pane's text so a later terminal can show it again.
//!
//! When the daemon restarts, a pane's process is gone, but what it printed is still
//! worth seeing. [`Snapshot`] is what the daemon saves; [`to_ansi`] turns its lines back
//! into terminal output (text plus SGR colour and style codes) that a fresh emulator is
//! fed before the new shell starts, so the old output sits above the new prompt.

use std::fmt::Write as _;

use crate::screen::{Color, Line, Style, TermSize, flags};

/// A pane's recent output, as returned by [`crate::SessionHandle::snapshot`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// The terminal's size.
    pub size: TermSize,
    /// Whether a full-screen application (alternate screen) is showing. Its screen is
    /// that application's, not the shell's history, so it isn't worth replaying.
    pub alt_screen: bool,
    /// Scrollback and screen lines, oldest first (see [`crate::Emulator::history`]).
    pub lines: Vec<Line>,
}

/// `lines` as terminal output: each line's text with its colours and styles, ending in
/// CR LF, and the style reset after every styled run. Control characters in the text
/// are dropped, so the result can't change terminal modes.
pub fn to_ansi(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        for span in &line.spans {
            let text: String = span.text.chars().filter(|c| !c.is_control()).collect();
            if span.style.is_default() {
                out.push_str(&text);
            } else {
                out.push_str(&sgr(span.style));
                out.push_str(&text);
                out.push_str("\x1b[0m");
            }
        }
        out.push_str("\r\n");
    }
    out
}

/// The SGR sequence that selects `style` from the default one.
fn sgr(style: Style) -> String {
    let mut codes: Vec<String> = vec!["0".into()];
    for (flag, code) in [
        (flags::BOLD, "1"),
        (flags::DIM, "2"),
        (flags::ITALIC, "3"),
        (flags::UNDERLINE, "4"),
        (flags::INVERSE, "7"),
        (flags::HIDDEN, "8"),
        (flags::STRIKETHROUGH, "9"),
    ] {
        if style.flags & flag != 0 {
            codes.push(code.into());
        }
    }
    push_color(&mut codes, style.fg, 30);
    push_color(&mut codes, style.bg, 40);
    let mut out = String::from("\x1b[");
    let _ = write!(out, "{}m", codes.join(";"));
    out
}

/// The codes for a foreground (`base` 30) or background (`base` 40) colour.
fn push_color(codes: &mut Vec<String>, color: Color, base: u8) {
    match color {
        Color::Default => {}
        Color::Indexed(n) if n < 8 => codes.push((base + n).to_string()),
        Color::Indexed(n) if n < 16 => codes.push((base + 60 + n - 8).to_string()),
        Color::Indexed(n) => codes.push(format!("{};5;{n}", base + 8)),
        Color::Rgb(r, g, b) => codes.push(format!("{};2;{r};{g};{b}", base + 8)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::{AlacrittyEmulator, Emulator};

    fn emulator_with(bytes: &str) -> AlacrittyEmulator {
        let mut e = AlacrittyEmulator::new(TermSize::new(40, 5));
        e.advance(bytes.as_bytes());
        e
    }

    #[test]
    fn replayed_output_looks_the_same() {
        let original = emulator_with(
            "plain\r\n\x1b[1;31mbold red\x1b[0m and \x1b[38;5;200;48;2;1;2;3mfancy\x1b[0m\r\n\
             \x1b[2;3;4;7;9mstyles\x1b[0m 漢字\r\n\x1b[92mbright\x1b[0m\r\n6\r\n7\r\n8\r\n",
        );
        let lines = original.history(100);
        assert_eq!(lines.len(), 7, "scrollback included, blank bottom dropped");
        let replayed = emulator_with(&to_ansi(&lines));
        assert_eq!(replayed.history(100), lines);
        assert_eq!(
            original.history(2),
            lines[5..].to_vec(),
            "only the last lines"
        );
    }

    #[test]
    fn control_characters_are_dropped() {
        let line = Line {
            spans: vec![crate::screen::Span {
                text: "a\x1b[?1049hb".into(),
                style: Style::default(),
            }],
        };
        assert_eq!(to_ansi(&[line]), "a[?1049hb\r\n");
    }
}
