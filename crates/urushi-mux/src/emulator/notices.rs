//! Terminal notifications, which alacritty's parser drops: iTerm2's `OSC 9 ; text`,
//! rxvt's `OSC 777 ; notify ; title ; body`, and Urushi's own `OSC 777 ; urushi ; done`
//! (see `urushi_core::attention`), including its `ready`, `running` and `idle` signals,
//! which are not attention.
//! A second, output-only parser watches the same bytes for them, so nothing about the
//! screen changes.

use alacritty_terminal::vte::{Parser, Perform};
use urushi_core::attention::{
    AttentionKind, IDLE_SIGNAL, READY_SIGNAL, RUNNING_SIGNAL, SIGNAL_NAMESPACE, SIGNAL_OSC,
};

/// A notification seen in the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    /// The program wants the user.
    Attention(AttentionKind),
    /// An agent's hook says it reads input now (`READY_SIGNAL`).
    Ready,
    /// An agent's hook says it works (`true`, `RUNNING_SIGNAL`) or its session ended
    /// (`false`, `IDLE_SIGNAL`).
    Running(bool),
}

/// Watches output for notifications. Keeps parser state between chunks, so a sequence
/// split across reads is still seen.
#[derive(Default)]
pub(super) struct Notices {
    parser: Parser,
}

impl Notices {
    /// The notifications in `bytes`, in order.
    pub(super) fn advance(&mut self, bytes: &[u8]) -> Vec<Notice> {
        let mut sink = Sink(Vec::new());
        self.parser.advance(&mut sink, bytes);
        sink.0
    }
}

struct Sink(Vec<Notice>);

impl Perform for Sink {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let Some(kind) = classify(params) {
            self.0.push(kind);
        }
    }
}

/// What an OSC sequence (split at `;`) asks for, if it's a notification.
fn classify(params: &[&[u8]]) -> Option<Notice> {
    match params {
        // iTerm2's notification. ConEmu uses `9 ; <number> ; …` for other things
        // (progress bars, working directory), which aren't notifications.
        [b"9", text, ..] => {
            let numeric = text.iter().all(u8::is_ascii_digit);
            (!text.is_empty() && !numeric).then_some(Notice::Attention(AttentionKind::Waiting))
        }
        [osc, name, word, ..]
            if *osc == SIGNAL_OSC.as_bytes() && *name == SIGNAL_NAMESPACE.as_bytes() =>
        {
            match *word {
                w if w == READY_SIGNAL.as_bytes() => return Some(Notice::Ready),
                w if w == RUNNING_SIGNAL.as_bytes() => return Some(Notice::Running(true)),
                w if w == IDLE_SIGNAL.as_bytes() => return Some(Notice::Running(false)),
                _ => {}
            }
            AttentionKind::from_signal(std::str::from_utf8(word).ok()?).map(Notice::Attention)
        }
        [b"777", b"notify", ..] => Some(Notice::Attention(AttentionKind::Waiting)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The attention asked for in `bytes`.
    fn seen(bytes: &[u8]) -> Vec<AttentionKind> {
        Notices::default()
            .advance(bytes)
            .into_iter()
            .filter_map(|n| match n {
                Notice::Attention(kind) => Some(kind),
                Notice::Ready | Notice::Running(_) => None,
            })
            .collect()
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
        assert_eq!(
            notices.advance(b"shi;done\x07"),
            vec![Notice::Attention(AttentionKind::Done)]
        );
    }

    #[test]
    fn the_ready_signal_is_seen_but_is_not_attention() {
        let ready = urushi_core::attention::signal_sequence(READY_SIGNAL);
        assert_eq!(
            Notices::default().advance(ready.as_bytes()),
            vec![Notice::Ready]
        );
        assert!(seen(ready.as_bytes()).is_empty());
    }

    #[test]
    fn running_and_idle_signals_are_seen_but_are_not_attention() {
        let running = urushi_core::attention::signal_sequence(RUNNING_SIGNAL);
        let idle = urushi_core::attention::signal_sequence(IDLE_SIGNAL);
        let both = format!("{running}output{idle}");
        assert_eq!(
            Notices::default().advance(both.as_bytes()),
            vec![Notice::Running(true), Notice::Running(false)]
        );
        assert!(seen(both.as_bytes()).is_empty());
        // Split across reads.
        let mut notices = Notices::default();
        let (a, b) = running.as_bytes().split_at(9);
        assert!(notices.advance(a).is_empty());
        assert_eq!(notices.advance(b), vec![Notice::Running(true)]);
    }
}
