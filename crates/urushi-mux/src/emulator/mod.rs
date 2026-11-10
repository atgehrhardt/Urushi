//! The VT emulator boundary.
//!
//! [`Emulator`] is all the session actor needs from a terminal emulator. The only
//! implementation today is [`AlacrittyEmulator`]. Replacing it (e.g. with an in-house
//! emulator) means implementing this trait. Nothing else changes.

mod alacritty;
mod notices;

pub use alacritty::AlacrittyEmulator;

use crate::screen::{Cursor, Line, Modes, TermSize};

/// Something the emulator wants done after processing output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmulatorEvent {
    /// Bytes to write back to the PTY (answers to queries such as cursor position reports).
    Reply(Vec<u8>),
    /// The application set (`Some`) or reset (`None`) the window title.
    Title(Option<String>),
    /// The application rang the bell.
    Bell,
    /// The application posted a terminal notification (OSC 9 or 777), or Urushi's own
    /// signal that an agent finished or waits (`urushi_core::attention`).
    Attention(urushi_core::AttentionKind),
    /// An agent's hook said it has started and reads input (Urushi's `ready` signal).
    /// Not attention.
    Ready,
    /// An agent's hook said it is working (`true`, Urushi's `running` signal) or that its
    /// session ended (`false`, `idle`). Not attention.
    Running(bool),
}

/// Which viewport lines changed since the last [`Emulator::take_damage`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Damage {
    /// Everything (resize, scroll, screen switch).
    Full,
    /// Only these viewport lines.
    Lines(Vec<u16>),
}

/// How to move the scrollback view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scroll {
    /// Positive moves back into history, negative moves towards the live view.
    Lines(i32),
    /// Return to the live view.
    Bottom,
}

/// A headless VT emulator: bytes in, screen state out.
pub trait Emulator: Send {
    /// Processes output from the PTY.
    fn advance(&mut self, bytes: &[u8]) -> Vec<EmulatorEvent>;
    /// Changes the grid size, reflowing as needed.
    fn resize(&mut self, size: TermSize);
    /// Current size.
    fn size(&self) -> TermSize;
    /// Returns which lines changed since the last call, and resets the tracking.
    fn take_damage(&mut self) -> Damage;
    /// Content of a viewport line (taking the scrollback offset into account).
    fn line(&self, viewport_line: u16) -> Line;
    /// Cursor in viewport coordinates.
    fn cursor(&self) -> Cursor;
    /// Input-relevant modes.
    fn modes(&self) -> Modes;
    /// Moves the scrollback view.
    fn scroll(&mut self, scroll: Scroll);
    /// Lines scrolled back from the live view.
    fn display_offset(&self) -> u32;
    /// Lines of scrollback available.
    fn history_size(&self) -> u32;
    /// The last `max_lines` lines of scrollback and screen, oldest first, as on the live
    /// view (whatever the scrollback position). Blank lines at the bottom are left out.
    fn history(&self, max_lines: usize) -> Vec<Line>;
}
