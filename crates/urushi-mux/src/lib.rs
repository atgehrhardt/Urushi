//! Urushi's headless terminal multiplexer.
//!
//! The multiplexer is invisible infrastructure (ADR 0001). It keeps each pane's process
//! and screen state alive in the daemon, so GUI clients can attach, detach and reattach
//! without losing anything. It has no UI of its own.
//!
//! - [`pty`]: the PTY boundary ([`Pty`] trait, [`NativePty`] via `portable-pty`).
//! - [`emulator`]: the VT emulator boundary ([`Emulator`] trait, [`AlacrittyEmulator`]).
//! - [`session`]: the per-pane actor tying them together ([`SessionHandle`]).
//! - [`screen`]: the renderer-neutral screen model that clients draw.
//! - [`keys`]: abstract key events → terminal bytes.
//! - [`replay`]: a pane's recent output, saved and replayed after a daemon restart.

pub mod emulator;
pub mod keys;
pub mod mouse;
mod platform;
pub mod pty;
pub mod replay;
pub mod screen;
pub mod session;

pub use emulator::{AlacrittyEmulator, Emulator, Scroll};
pub use keys::{Key, KeyEvent, Modifiers};
pub use mouse::{MouseAction, MouseButton, MouseEvent, WheelEvent};
pub use pty::{NativePty, Pty, SpawnSpec};
pub use replay::Snapshot;
pub use screen::{ScreenFrame, TermSize};
pub use session::{Input, SessionHandle, SessionStatus};

use thiserror::Error;

/// Multiplexer failures.
#[derive(Debug, Error)]
pub enum MuxError {
    /// The process or PTY couldn't be created.
    #[error("failed to spawn process: {0}")]
    Spawn(String),
    /// An OS-level failure (e.g. creating a thread).
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The session has stopped.
    #[error("session has ended")]
    Closed,
}
