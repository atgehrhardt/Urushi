//! The notes editor: a thread's notes in Markdown over several lines, with formatting
//! controls above and the notes' color below (ADR 0058), saved with Save (or
//! Cmd/Ctrl+Enter) and kept by the daemon (ADR 0048).

use urushi_ui::widgets::styled_text_area;
use urushi_ui::{Align, Length, Node, TextField, Theme};

use super::dialogs::{cancel, field_key, hint, label};
use super::modal::modal;
use crate::msg::{Msg, ThreadMsg};
use crate::state::{AppState, Field};

mod toolbar;

/// How many lines the notes field shows.
const LINES: u8 = 10;

/// The dialog for `thread`'s `notes`, in the wire `color`.
pub(super) fn dialog(
    state: &AppState,
    theme: &Theme,
    thread: &str,
    (notes, color): (&TextField, i32),
) -> Node<Msg> {
    let title = state
        .workspace
        .thread(thread)
        .map_or_else(|| "Notes".to_owned(), |t| format!("Notes — {}", t.title));
    let save = if cfg!(any(target_os = "macos", target_os = "ios")) {
        "⌘↩ saves · ⌘B bold · ⌘I italic · ⌘K link"
    } else {
        "Ctrl+Enter saves · Ctrl+B bold · Ctrl+I italic · Ctrl+K link"
    };
    let runs = super::note_style::editor_runs(theme, notes.text());
    let area = styled_text_area(
        theme,
        notes,
        state.focus == Some(Field::ThreadNotes),
        (
            field_key(Field::ThreadNotes),
            "What to remember about this thread…",
            Msg::FocusField(Field::ThreadNotes),
        ),
        (LINES, runs),
    )
    .on_secondary_click(Msg::field_menu(Field::ThreadNotes));
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(toolbar::toolbar(theme))
        .child(area)
        .children(
            state
                .daemon_supports_notes_color()
                .then(|| color_row(theme, color)),
        )
        .child(hint(theme, save));
    modal(
        state,
        theme,
        &title,
        480.0,
        body,
        vec![
            cancel(theme),
            super::modal::submit_button(state, theme, "Save"),
        ],
        Some(Msg::CloseDialog),
    )
}

/// "Color" and the palette, the notes' color ringed (yellow until one is picked).
fn color_row(theme: &Theme, color: i32) -> Node<Msg> {
    let chosen = super::note_style::shown_color(color);
    Node::row()
        .gap(8.0)
        .align(Align::Center)
        .child(label(theme, "Color"))
        .child(super::labels::palette_swatches(theme, chosen, |wire| {
            Msg::Thread(ThreadMsg::NotesColor(wire))
        }))
}
