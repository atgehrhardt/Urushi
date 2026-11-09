//! Messages from a pane's context menu (opened with a secondary click, i.e. a right or
//! two-finger click, over the pane). Split and close name their pane, so the menu acts
//! on the pane it was opened on even if focus moves while it is open.

use urushi_client::v1;

/// Something done to a pane from its menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneMsg {
    /// Split a pane; the new pane goes on `side`.
    Split {
        /// Pane id.
        pane: String,
        /// Where the new pane goes.
        side: v1::Direction,
    },
    /// Close a pane.
    Close(String),
    /// Copy the selected text.
    Copy,
    /// Paste the clipboard into the focused pane (the menu's, which a secondary click
    /// focuses).
    Paste,
}
