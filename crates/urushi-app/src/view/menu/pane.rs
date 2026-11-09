//! A terminal pane's menu, opened by a right or two-finger click on the pane: split it
//! right or down, copy and paste, close it. The hints are the keyboard shortcuts that do
//! the same to the focused pane.

use urushi_client::v1;
use urushi_ui::{Node, Theme};

use super::{item, separator};
use crate::msg::{Msg, PaneMsg};
use crate::state::AppState;

pub(super) fn pane_items(state: &AppState, theme: &Theme, pane: &str) -> Vec<Node<Msg>> {
    let selected = state.selection.as_ref().is_some_and(|(p, _)| p == pane);
    let copy = if selected {
        theme.text
    } else {
        theme.text_muted
    };
    let split = |side| {
        Msg::Pane(PaneMsg::Split {
            pane: pane.to_owned(),
            side,
        })
    };
    vec![
        item(
            theme,
            "Split right",
            &hint("D", false),
            theme.text,
            split(v1::Direction::Right),
        ),
        item(
            theme,
            "Split down",
            &down_hint(),
            theme.text,
            split(v1::Direction::Down),
        ),
        separator(theme),
        item(
            theme,
            "Copy",
            &hint("C", false),
            copy,
            Msg::Pane(PaneMsg::Copy),
        ),
        item(
            theme,
            "Paste",
            &hint("V", false),
            theme.text,
            Msg::Pane(PaneMsg::Paste),
        ),
        separator(theme),
        item(
            theme,
            "Close pane",
            &hint("W", false),
            theme.text,
            Msg::Pane(PaneMsg::Close(pane.to_owned())),
        ),
    ]
}

/// The app's shortcut for `letter`: ⌘ on Apple platforms, Ctrl+Shift elsewhere (where
/// Ctrl alone belongs to the terminal).
fn hint(letter: &str, shift: bool) -> String {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        format!("{}⌘{letter}", if shift { "⇧" } else { "" })
    } else {
        format!("Ctrl+Shift+{letter}")
    }
}

/// Split down is ⇧⌘D on Apple platforms and Ctrl+Shift+E elsewhere.
fn down_hint() -> String {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        hint("D", true)
    } else {
        hint("E", false)
    }
}
