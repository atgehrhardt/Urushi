//! Attention: a pane whose agent finished its response, or that waits for the user
//! (a permission prompt, a question, or any program ringing the bell or posting a
//! terminal notification).
//!
//! Attention is raised by what the pane's program writes to its terminal, and cleared
//! when the user types into the pane or looks at it. Urushi's own signal is a private
//! OSC 777 sequence, [`AttentionKind::signal_sequence`]. Agents started by Urushi get
//! hooks that write it (`agent::hooks`); the daemon's terminal emulator recognises it.

/// Why a pane wants the user. `Waiting` outranks `Done`: a thread shows its most urgent
/// pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttentionKind {
    /// An agent finished its response.
    Done,
    /// A program waits for the user: a permission prompt, a question, a bell.
    Waiting,
}

/// The OSC number of Urushi's signal (rxvt's "extension" OSC: `777 ; <name> ; …`).
pub const SIGNAL_OSC: &str = "777";
/// The extension name in Urushi's signal, so it can't be mistaken for `777;notify`.
pub const SIGNAL_NAMESPACE: &str = "urushi";

impl AttentionKind {
    /// The word for this kind in Urushi's signal.
    pub const fn signal(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Waiting => "waiting",
        }
    }

    /// The kind a signal word names.
    pub fn from_signal(word: &str) -> Option<Self> {
        match word {
            "done" => Some(Self::Done),
            "waiting" => Some(Self::Waiting),
            _ => None,
        }
    }

    /// The escape sequence a program writes to its terminal to raise this:
    /// `ESC ] 777 ; urushi ; done BEL`.
    pub fn signal_sequence(self) -> String {
        format!(
            "\u{1b}]{SIGNAL_OSC};{SIGNAL_NAMESPACE};{}\u{7}",
            self.signal()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_outranks_done() {
        assert!(AttentionKind::Waiting > AttentionKind::Done);
        assert_eq!(
            [AttentionKind::Waiting, AttentionKind::Done].iter().max(),
            Some(&AttentionKind::Waiting)
        );
    }

    #[test]
    fn signals_name_their_kind() {
        for kind in [AttentionKind::Done, AttentionKind::Waiting] {
            assert_eq!(AttentionKind::from_signal(kind.signal()), Some(kind));
        }
        assert_eq!(AttentionKind::from_signal("nope"), None);
        assert_eq!(
            AttentionKind::Done.signal_sequence(),
            "\u{1b}]777;urushi;done\u{7}"
        );
    }
}
