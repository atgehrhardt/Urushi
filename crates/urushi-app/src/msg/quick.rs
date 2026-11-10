//! Messages for the quick terminal's drop-down panel (ADR 0031).

use urushi_ui::Rect;

/// Something done to the quick-terminal panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuickMsg {
    /// Drop the panel down (opening the daemon's quick terminal if there is none), or
    /// put it away. Its programs keep running either way.
    Toggle,
    /// Put the panel away (a click below it).
    Hide,
    /// The slide animation advanced (0 = hidden, 1 = shown).
    Reveal(f32),
    /// Where the panel's terminal area is once fully down (the runner reports it).
    Area(Rect),
}
