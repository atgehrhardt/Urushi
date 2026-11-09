//! Terminal notifications, which alacritty's parser drops: iTerm2's `OSC 9 ; text`,
//! rxvt's `OSC 777 ; notify ; title ; body`, and Urushi's own `OSC 777 ; urushi ; done`
//! (see `urushi_core::attention`). A second, output-only parser watches the same bytes
//! for them, so nothing about the screen changes.

use alacritty_terminal::vte::{Parser, Perform};
use urushi_core::attention::{AttentionKind, SIGNAL_NAMESPACE, SIGNAL_OSC};

/// Watches output for notifications. Keeps parser state between chunks, so a sequence
/// split across reads is still seen.
#[derive(Default)]
pub(super) struct Notices {
    parser: Parser,
}

impl Notices {
    /// The notifications in `bytes`, in order.
    pub(super) fn advance(&mut self, bytes: &[u8]) -> Vec<AttentionKind> {
        let mut sink = Sink(Vec::new());
        self.parser.advance(&mut sink, bytes);
        sink.0
    }
}

struct Sink(Vec<AttentionKind>);

impl Perform for Sink {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let Some(kind) = classify(params) {
            self.0.push(kind);
        }
    }
}

/// What an OSC sequence (split at `;`) asks for, if it's a notification.
fn classify(params: &[&[u8]]) -> Option<AttentionKind> {
    match params {
        // iTerm2's notification. ConEmu uses `9 ; <number> ; …` for other things
        // (progress bars, working directory), which aren't notifications.
        [b"9", text, ..] => {
            let numeric = text.iter().all(u8::is_ascii_digit);
            (!text.is_empty() && !numeric).then_some(AttentionKind::Waiting)
        }
        [osc, name, word, ..]
            if *osc == SIGNAL_OSC.as_bytes() && *name == SIGNAL_NAMESPACE.as_bytes() =>
        {
            AttentionKind::from_signal(std::str::from_utf8(word).ok()?)
        }
        [b"777", b"notify", ..] => Some(AttentionKind::Waiting),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(bytes: &[u8]) -> Vec<AttentionKind> {
        Notices::default().advance(bytes)
    }

    #[test]
    fn urushi_signals_are_recognised_with_either_terminator() {
        assert_eq!(
            seen(AttentionKind::Done.signal_sequence().as_bytes()),
            vec![AttentionKind::Done]
        );
        assert_eq!(
            seen(b"x\x1b]777;urushi;waiting\x1b\\y"),
            vec![AttentionKind::Waiting]
        );
        assert!(seen(b"\x1b]777;urushi;bogus\x07").is_empty());
    }

    #[test]
    fn terminal_notifications_mean_waiting() {
        assert_eq!(
            seen(b"\x1b]9;Approval requested: rm -rf build\x07"),
            vec![AttentionKind::Waiting]
        );
        assert_eq!(
            seen(b"\x1b]777;notify;Build;done\x07"),
            vec![AttentionKind::Waiting]
        );
    }

    #[test]
    fn other_sequences_are_not_notifications() {
        // ConEmu progress, a title, a hyperlink, plain text.
        assert!(seen(b"\x1b]9;4;1;50\x07").is_empty());
        assert!(seen(b"\x1b]0;my title\x07").is_empty());
        assert!(seen(b"\x1b]8;;https://x.y\x07link\x1b]8;;\x07").is_empty());
        assert!(seen(b"hello \x1b[31mred\x1b[0m").is_empty());
    }

    #[test]
    fn a_sequence_split_across_reads_is_seen_once() {
        let mut notices = Notices::default();
        assert!(notices.advance(b"\x1b]777;uru").is_empty());
        assert_eq!(notices.advance(b"shi;done\x07"), vec![AttentionKind::Done]);
    }
}
