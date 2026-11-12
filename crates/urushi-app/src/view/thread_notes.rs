//! The notes editor: a thread's notes over several lines, saved with Save (or
//! Cmd/Ctrl+Enter) and kept by the daemon (ADR 0048).

use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Length, Node, TextField, Theme};

use super::dialogs::{area, cancel, hint};
use super::modal::modal;
use crate::msg::Msg;
use crate::state::{AppState, Field};

/// How many lines the notes field shows.
const LINES: u8 = 10;

/// The dialog for `thread`'s `notes`.
pub(super) fn dialog(
    state: &AppState,
    theme: &Theme,
    thread: &str,
    notes: &TextField,
) -> Node<Msg> {
    let title = state
        .workspace
        .thread(thread)
        .map_or_else(|| "Notes".to_owned(), |t| format!("Notes — {}", t.title));
    let save = if cfg!(any(target_os = "macos", target_os = "ios")) {
        "⌘↩ saves"
    } else {
        "Ctrl+Enter saves"
    };
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(area(
            state,
            theme,
            notes,
            Field::ThreadNotes,
            ("What to remember about this thread…", LINES),
        ))
        .child(hint(theme, save));
    modal(
        state,
        theme,
        &title,
        480.0,
        body,
        vec![
            cancel(theme),
            button(theme, "Save", ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}
