//! Attention: a pane whose agent finished its response, or that waits for the user
//! (a permission prompt, a question, or any program ringing the bell or posting a
//! terminal notification).
//!
//! Attention is raised by what the pane's program writes to its terminal, and cleared
//! when the user types into the pane or looks at it. Urushi's own signal is a private
//! OSC 777 sequence, [`AttentionKind::signal_sequence`]. Agents started by Urushi get
//! hooks that write it (`agent::hooks`); the daemon's terminal emulator recognises it.
//!
//! The same sequence also carries signals that are not attention:
//! - [`READY_SIGNAL`]: an agent's hook says its input is ready, so a drafted task message
//!   can be typed into it (ADR 0030).
//! - [`RUNNING_SIGNAL`] and [`IDLE_SIGNAL`]: an agent's hooks say it is working on a
//!   turn (a prompt, a tool, a subagent) or has ended its session (ADR 0039).
//! - [`BACKGROUND_SIGNAL`]: an agent's hook says how many shells and monitors it left
//!   running in the background ([`BackgroundTasks`], ADR 0059).

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
/// The word in Urushi's signal for "the agent has started and reads input". Never raises
/// attention.
pub const READY_SIGNAL: &str = "ready";
/// The word in Urushi's signal for "the agent is working": it took a prompt, uses a tool
/// or started a subagent. Never raises attention.
pub const RUNNING_SIGNAL: &str = "running";
/// The word in Urushi's signal for "the agent's session ended": it no longer works,
/// though nothing finished. Never raises attention.
pub const IDLE_SIGNAL: &str = "idle";

/// The word in Urushi's signal for "these background tasks are open":
/// `background ; <shells> ; <monitors>`. Never raises attention.
pub const BACKGROUND_SIGNAL: &str = "background";

/// The shells and monitors an agent keeps running in the background (Claude's `shell`
/// and `monitor` background tasks), as its latest report says (ADR 0059). They outlive
/// the agent's turn: a dev server or a watch stays open after the agent finished.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BackgroundTasks {
    /// Background shells (commands run in the background).
    pub shells: u32,
    /// Monitors (background watches that report events to the agent).
    pub monitors: u32,
}

impl BackgroundTasks {
    /// Whether nothing is open.
    pub const fn is_empty(self) -> bool {
        self.shells == 0 && self.monitors == 0
    }

    /// These counts added to `other`'s (for a thread with several panes).
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            shells: self.shells.saturating_add(other.shells),
            monitors: self.monitors.saturating_add(other.monitors),
        }
    }

    /// The escape sequence reporting these counts:
    /// `ESC ] 777 ; urushi ; background ; <shells> ; <monitors> BEL`.
    pub fn signal_sequence(self) -> String {
        signal_sequence(&format!(
            "{BACKGROUND_SIGNAL};{};{}",
            self.shells, self.monitors
        ))
    }

    /// The counts in the parameters after [`BACKGROUND_SIGNAL`]. A missing or
    /// unreadable count is 0, so a garbled report never invents open tasks.
    pub fn from_params(params: &[&[u8]]) -> Self {
        let count = |i: usize| {
            params
                .get(i)
                .and_then(|p| std::str::from_utf8(p).ok())
                .and_then(|p| p.trim().parse().ok())
                .unwrap_or(0)
        };
        Self {
            shells: count(0),
            monitors: count(1),
        }
    }
}

/// Urushi's signal carrying `word`: `ESC ] 777 ; urushi ; <word> BEL`.
pub fn signal_sequence(word: &str) -> String {
    format!("\u{1b}]{SIGNAL_OSC};{SIGNAL_NAMESPACE};{word}\u{7}")
}

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
        signal_sequence(self.signal())
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
        for word in [READY_SIGNAL, RUNNING_SIGNAL, IDLE_SIGNAL, BACKGROUND_SIGNAL] {
            assert_eq!(
                AttentionKind::from_signal(word),
                None,
                "{word} is not attention"
            );
        }
        assert_eq!(
            signal_sequence(READY_SIGNAL),
            "\u{1b}]777;urushi;ready\u{7}"
        );
        assert_eq!(
            AttentionKind::Done.signal_sequence(),
            "\u{1b}]777;urushi;done\u{7}"
        );
    }

    #[test]
    fn background_tasks_round_trip_through_their_signal() {
        let tasks = BackgroundTasks {
            shells: 2,
            monitors: 1,
        };
        assert_eq!(
            tasks.signal_sequence(),
            "\u{1b}]777;urushi;background;2;1\u{7}"
        );
        assert_eq!(BackgroundTasks::from_params(&[b"2", b"1"]), tasks);
        assert!(!tasks.is_empty());
        assert!(BackgroundTasks::default().is_empty());
        assert_eq!(
            tasks.plus(BackgroundTasks {
                shells: 1,
                monitors: 0
            }),
            BackgroundTasks {
                shells: 3,
                monitors: 1
            }
        );
    }

    #[test]
    fn unreadable_background_counts_are_zero() {
        assert!(BackgroundTasks::from_params(&[]).is_empty());
        assert!(BackgroundTasks::from_params(&[b"x", b"-1"]).is_empty());
        assert_eq!(
            BackgroundTasks::from_params(&[b"3"]),
            BackgroundTasks {
                shells: 3,
                monitors: 0
            }
        );
        // `wc -l` pads its count on macOS.
        assert_eq!(BackgroundTasks::from_params(&[b"  4", b"0"]).shells, 4);
    }
}
