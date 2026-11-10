//! Client-side state of the quick terminal's drop-down panel (ADR 0031): whether it's
//! down (and how far it has slid), and where its terminals are. The quick terminal
//! itself (its panes and their processes) lives in the daemon and arrives with the
//! workspace (`Workspace.quick_terminal`); putting the panel away only detaches it.

use urushi_ui::Rect;

pub use urushi_client::QUICK_TERMINAL_SINCE_MINOR;

/// How much of the main area the panel covers when it's down.
pub const HEIGHT_SHARE: f32 = 0.6;

/// How long the panel takes to drop down or go back up, in seconds.
pub const SLIDE_SECONDS: f32 = crate::board::SLIDE_SECONDS;

/// The quick-terminal panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuickPanel {
    /// Down (or dropping). While down, typing goes to the quick terminal.
    pub open: bool,
    /// How far it has dropped: 0 = hidden, 1 = fully down. The runner animates this
    /// towards `open`.
    pub reveal: f32,
    /// The terminal area when fully down, as last reported. Kept while the panel is
    /// away, so reopening attaches at once at the size it had.
    pub area: Option<Rect>,
    /// An `OpenQuickTerminal` request is on its way (one at a time).
    pub starting: bool,
}

impl QuickPanel {
    /// Whether any of the panel is on screen.
    pub fn visible(&self) -> bool {
        self.open || self.reveal > 0.0
    }

    /// `reveal` with an ease-out curve, for drawing.
    pub fn eased(&self) -> f32 {
        crate::board::ease_out(self.reveal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_is_visible_while_it_moves() {
        let mut panel = QuickPanel::default();
        assert!(!panel.visible());
        panel.open = true;
        assert!(panel.visible());
        panel.open = false;
        panel.reveal = 0.3;
        assert!(panel.visible());
        assert!(panel.eased() > 0.3, "eases out");
    }
}
