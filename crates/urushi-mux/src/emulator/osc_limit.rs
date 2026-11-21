//! A cap on how long an OSC string may grow before the parsers see it.
//!
//! vte, as alacritty builds it, buffers an OSC payload without limit, and alacritty
//! keeps up to 4096 copies of the title on its title stack (`CSI 22 t`). A program in a
//! pane, or `cat` of a crafted file, could then make the daemon hold gigabytes: a 1 MB
//! title pushed 4096 times. [`OscLimit`] sits in front of both parsers and drops OSC
//! payload bytes past [`MAX_OSC_PAYLOAD`], so a title or notification is cut short and
//! every other byte passes through unchanged.
//!
//! It follows vte's own state changes closely enough to know when an OSC string starts
//! (`ESC ]`) and ends (BEL, CAN, SUB or ESC), across reads.

/// The most payload bytes an OSC string keeps. Titles and notifications are far
/// shorter; with the title stack's depth this bounds it at 16 MiB per pane.
pub(super) const MAX_OSC_PAYLOAD: usize = 4096;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    /// Anything other than the two below: ground, CSI, DCS and so on. Only ESC matters.
    #[default]
    Other,
    /// Just after ESC, where `]` starts an OSC string.
    Escape,
    /// Inside an OSC string, with this many payload bytes so far.
    Osc(usize),
}

/// Truncates OSC strings in a byte stream. Keeps its state between calls, so a string
/// split across reads is still capped.
#[derive(Debug, Default)]
pub(super) struct OscLimit {
    state: State,
}

impl OscLimit {
    /// `bytes` with any OSC payload beyond [`MAX_OSC_PAYLOAD`] removed. Borrows the
    /// input when nothing is removed, which is nearly always.
    pub(super) fn filter<'a>(&mut self, bytes: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
        let mut kept: Option<Vec<u8>> = None;
        for (i, &byte) in bytes.iter().enumerate() {
            let keep = self.step(byte);
            match (&mut kept, keep) {
                (Some(out), true) => out.push(byte),
                (None, false) => kept = Some(bytes[..i].to_vec()),
                _ => {}
            }
        }
        kept.map_or(std::borrow::Cow::Borrowed(bytes), std::borrow::Cow::Owned)
    }

    /// Advances over one byte; returns whether to keep it.
    fn step(&mut self, byte: u8) -> bool {
        let (next, keep) = match (self.state, byte) {
            (State::Escape, b']') => (State::Osc(0), true),
            // ESC starts an escape from anywhere (ending an OSC string). C0 controls
            // other than CAN and SUB run without leaving the escape state, as in vte.
            (_, ESC) | (State::Escape, 0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f) => {
                (State::Escape, true)
            }
            (State::Osc(n), _) if !matches!(byte, BEL | CAN | SUB) => {
                (State::Osc(n.saturating_add(1)), n < MAX_OSC_PAYLOAD)
            }
            // The end of a string (BEL, CAN, SUB) or of an escape.
            _ => (State::Other, true),
        };
        self.state = next;
        keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(limit: &mut OscLimit, bytes: &[u8]) -> Vec<u8> {
        limit.filter(bytes).into_owned()
    }

    #[test]
    fn ordinary_output_passes_through_untouched() {
        let mut limit = OscLimit::default();
        let text =
            b"hello \x1b[31mred\x1b[0m \x1b]0;title\x07 \x1b]777;urushi;done\x1b\\ \xe2\x9c\x93";
        assert!(matches!(limit.filter(text), std::borrow::Cow::Borrowed(_)));
        assert_eq!(run(&mut limit, text), text);
    }

    #[test]
    fn a_long_title_is_cut_at_the_cap_and_what_follows_survives() {
        let mut limit = OscLimit::default();
        let mut bytes = b"\x1b]2;".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 1_000_000));
        bytes.extend(b"\x07after");
        let out = run(&mut limit, &bytes);
        let mut expected = b"\x1b]2;".to_vec();
        expected.extend(std::iter::repeat_n(b'x', MAX_OSC_PAYLOAD - 2));
        expected.extend(b"\x07after");
        assert_eq!(out, expected);
    }

    #[test]
    fn the_cap_holds_across_reads() {
        let mut limit = OscLimit::default();
        let mut total = run(&mut limit, b"\x1b");
        total.extend(run(&mut limit, b"]0;"));
        for _ in 0..100 {
            total.extend(run(&mut limit, &[b'y'; 1000]));
        }
        total.extend(run(&mut limit, b"\x1b\\ok"));
        assert_eq!(
            total.len(),
            2 + MAX_OSC_PAYLOAD + 4,
            "ESC ] + payload + ST + ok"
        );
        assert!(total.ends_with(b"\x1b\\ok"));
    }

    #[test]
    fn an_unterminated_string_ends_at_can_or_a_new_escape() {
        let mut limit = OscLimit::default();
        let mut bytes = b"\x1b]0;".to_vec();
        bytes.extend(std::iter::repeat_n(b'z', 10_000));
        let tail = b"\x18plain text that is long enough to matter";
        bytes.extend(tail);
        let out = run(&mut limit, &bytes);
        assert!(out.ends_with(tail));
        assert_eq!(out.len(), 2 + MAX_OSC_PAYLOAD + tail.len());
    }

    #[test]
    fn csi_with_a_bracket_is_not_osc() {
        let mut limit = OscLimit::default();
        let mut bytes = b"\x1b[".to_vec();
        bytes.extend(b"]");
        bytes.extend(std::iter::repeat_n(b'a', 10_000));
        assert_eq!(run(&mut limit, &bytes), bytes);
    }
}
