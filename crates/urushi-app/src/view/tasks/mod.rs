//! The task editor: title, notes, subtasks, column, color and tags, and starting a
//! thread for the task or a subtask.

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, modal};
use urushi_ui::{Align, Color, Font, Insets, Key, Length, Node, Role, TextField, Theme, key};

use super::dialogs::{area, cancel, field, hint, label};
use crate::board::TaskLabels;
use crate::msg::Msg;
use crate::state::{AppState, Field, SubtaskEditor, TaskLink};

mod labels;
mod subtasks;
/// Lines of notes shown before the notes scroll.
const NOTES_LINES: u8 = 5;

/// Key of the editor's scrolling subtask list.
pub(crate) fn subtasks_scroll_key() -> Key {
    key(&["task-subtasks"])
}

/// Width of the editor with its two columns (desktop).
const WIDE: f32 = 780.0;
/// Width of the one-column editor (phones).
const NARROW: f32 = 520.0;
/// Height of the one-column editor's scrolling body (phones).
const NARROW_BODY: f32 = 560.0;

/// Key of the one-column editor's scrolling body.
pub(crate) fn editor_scroll_key() -> Key {
    key(&["task-editor"])
}

/// Creating (`task: None`) or editing a task. On a desktop the text (title, notes,
/// subtasks) is on the left and the organizing (column, color, tags) on the right; on a
/// phone they are one scrolling column.
pub(super) fn editor(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    (board, column, task): (&str, &str, Option<&str>),
    (title, notes, subtasks, labels): (&TextField, &TextField, &SubtaskEditor, &TaskLabels),
) -> Node<Msg> {
    let mut text = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Title"))
        .child(field(
            state,
            theme,
            title,
            Field::TaskTitle,
            "What needs doing?",
        ))
        .child(label(theme, "Notes"))
        .child(area(
            state,
            theme,
            notes,
            Field::TaskNotes,
            ("Details, links… (optional)", NOTES_LINES),
        ));
    let current = task
        .and_then(|t| state.workspace.task(t))
        .map(|(_, _, t)| t);
    if state.daemon_supports_subtasks() {
        let offset = scroll.get(&subtasks_scroll_key()).copied().unwrap_or(0.0);
        text = text.children(subtasks::checklist(state, theme, current, subtasks, offset));
    }
    let mut organize = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Column"))
        .child(column_chips(state, theme, board, column, side_width(state)));
    if state.daemon_supports_board_extras() {
        organize = organize.children(labels::section(
            state,
            theme,
            (board, side_width(state)),
            labels,
        ));
    }
    let hint_text = if state.daemon_supports_subtasks() {
        format!(
            "Enter adds a subtask, a tag or a notes line, or saves from the title. {} saves.",
            save_keys()
        )
    } else {
        format!(
            "Tab moves between fields. Enter adds a line in notes; {} saves.",
            save_keys()
        )
    };
    let (body, width) = if state.compact {
        let offset = scroll.get(&editor_scroll_key()).copied().unwrap_or(0.0);
        let body = Node::column()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(NARROW_BODY))
            .gap(14.0)
            .scroll(editor_scroll_key(), offset)
            .child(text)
            .child(organize)
            .child(hint(theme, &hint_text));
        (body, NARROW)
    } else {
        let columns = Node::row()
            .width(Length::Fill(1.0))
            .gap(24.0)
            .align(Align::Start)
            .child(text.width(Length::Fill(1.4)))
            .child(organize.width(Length::Fill(1.0)));
        let body = Node::column()
            .width(Length::Fill(1.0))
            .gap(12.0)
            .child(columns)
            .child(hint(theme, &hint_text));
        (body, WIDE)
    };
    let actions = editor_actions(state, theme, current, task.is_some());
    modal(
        theme,
        if task.is_some() {
            "Edit task"
        } else {
            "New task"
        },
        width,
        body,
        actions,
        Some(Msg::CloseDialog),
    )
}

/// The keys that save from any field, notes included.
fn save_keys() -> &'static str {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        "⌘Enter"
    } else {
        "Ctrl+Enter"
    }
}

/// The column picker: one chip per column of the board.
fn column_chips(
    state: &AppState,
    theme: &Theme,
    board: &str,
    column: &str,
    width: f32,
) -> Node<Msg> {
    let columns = state
        .workspace
        .board(board)
        .map(|b| b.columns.as_slice())
        .unwrap_or_default();
    let chips = columns.iter().map(|c| {
        let chosen = c.id == column;
        let chip = Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .radius(theme.radius)
            .border(1.0, if chosen { theme.accent } else { theme.border })
            .background(if chosen {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(theme.hover)
            .key(key(&["task-column", &c.id]))
            .on_click(Msg::PickColumn(c.id.clone()))
            .role(Role::Item, format!("Column {}", c.name))
            .child(Node::text(
                &c.name,
                Font::UiMedium,
                theme.font_size_small,
                if chosen { theme.text } else { theme.text_muted },
            ));
        let size = theme.font_size_small;
        (super::labels::text_width(&c.name, size) + 22.0, chip)
    });
    super::labels::wrap(chips.collect(), 6.0, width)
}

/// The width of the editor's right-hand column (the whole body on phones), for
/// wrapping chips.
fn side_width(state: &AppState) -> f32 {
    if state.compact {
        NARROW - 40.0
    } else {
        (WIDE - 40.0 - 24.0) / 2.4
    }
}

/// Delete and Start thread (for an existing task), Cancel, and Save or Add task.
fn editor_actions(
    state: &AppState,
    theme: &Theme,
    current: Option<&v1::Task>,
    editing: bool,
) -> Vec<Node<Msg>> {
    let mut actions = Vec::new();
    if let Some(t) = current {
        actions.push(button(
            theme,
            "Delete task",
            ButtonKind::Danger,
            Msg::DeleteTask,
        ));
        actions.push(Node::spacer());
        if state.daemon_supports_subtasks() {
            let link = TaskLink {
                task: t.id.clone(),
                subtask: None,
            };
            let label = if state.linked_thread(&link).is_some() {
                "Go to thread"
            } else {
                "Start thread"
            };
            actions.push(button(
                theme,
                label,
                ButtonKind::Secondary,
                Msg::StartThread(link),
            ));
        }
    }
    actions.push(cancel(theme));
    actions.push(button(
        theme,
        if editing { "Save" } else { "Add task" },
        ButtonKind::Primary,
        Msg::Submit,
    ));
    actions
}
