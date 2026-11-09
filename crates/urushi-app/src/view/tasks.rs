//! Task-board dialogs: the task editor (title, notes, column, subtasks, and starting a
//! thread for the task or a subtask) and the confirmation before deleting a board.

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, icon_button, modal};
use urushi_ui::{Align, Color, Font, Insets, Key, Length, Node, Role, TextField, Theme, key};

use super::dialogs::{area, cancel, field, hint, label};
use crate::msg::Msg;
use crate::state::{AppState, Field, SubtaskEditor, TaskLink};

/// Subtask rows shown before the list scrolls.
const VISIBLE_SUBTASKS: usize = 6;
/// Height of one subtask row.
const SUBTASK_ROW: f32 = 28.0;
/// Lines of notes shown before the notes scroll.
const NOTES_LINES: u8 = 5;

/// Key of the editor's scrolling subtask list.
pub(crate) fn subtasks_scroll_key() -> Key {
    key(&["task-subtasks"])
}

/// Creating (`task: None`) or editing a task.
pub(super) fn editor(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    (board, column, task): (&str, &str, Option<&str>),
    (title, notes, subtasks): (&TextField, &TextField, &SubtaskEditor),
) -> Node<Msg> {
    let mut body = Node::column()
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
        ))
        .child(label(theme, "Column"))
        .child(column_chips(state, theme, board, column));
    let current = task
        .and_then(|t| state.workspace.task(t))
        .map(|(_, _, t)| t);
    if state.daemon_supports_subtasks() {
        let offset = scroll.get(&subtasks_scroll_key()).copied().unwrap_or(0.0);
        body = body.children(checklist(state, theme, current, subtasks, offset));
        body = body.child(hint(
            theme,
            &format!(
                "Enter adds a subtask or a notes line, or saves from the title. {} saves.",
                save_keys()
            ),
        ));
    } else {
        body = body.child(hint(
            theme,
            &format!(
                "Tab moves between fields. Enter adds a line in notes; {} saves.",
                save_keys()
            ),
        ));
    }
    let actions = editor_actions(state, theme, current, task.is_some());
    modal(
        theme,
        if task.is_some() {
            "Edit task"
        } else {
            "New task"
        },
        520.0,
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
fn column_chips(state: &AppState, theme: &Theme, board: &str, column: &str) -> Node<Msg> {
    let columns = state
        .workspace
        .board(board)
        .map(|b| b.columns.as_slice())
        .unwrap_or_default();
    let chips = columns.iter().map(|c| {
        let chosen = c.id == column;
        Node::row()
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
            ))
    });
    Node::row().gap(6.0).children(chips)
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

/// The "Subtasks" heading, the list (checked off and renamed in place, or drafts for a
/// new task), and the field that adds one.
fn checklist(
    state: &AppState,
    theme: &Theme,
    task: Option<&v1::Task>,
    editor: &SubtaskEditor,
    offset: f32,
) -> Vec<Node<Msg>> {
    let mut rows: Vec<Node<Msg>> = Vec::new();
    let heading = match task {
        Some(t) if !t.subtasks.is_empty() => {
            let done = t.subtasks.iter().filter(|s| s.done).count();
            format!("Subtasks  {done}/{}", t.subtasks.len())
        }
        _ => "Subtasks".to_owned(),
    };
    match task {
        Some(t) => {
            for s in &t.subtasks {
                rows.push(subtask_row(state, theme, t, s, editor));
            }
        }
        None => {
            for (i, title) in editor.drafts.iter().enumerate() {
                rows.push(draft_row(theme, i, title));
            }
        }
    }
    let mut out = vec![label(theme, &heading)];
    if !rows.is_empty() {
        let mut list = Node::column().width(Length::Fill(1.0)).gap(0.0);
        if rows.len() > VISIBLE_SUBTASKS {
            #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
            let height = VISIBLE_SUBTASKS as f32 * SUBTASK_ROW;
            list = list
                .height(Length::Fixed(height))
                .scroll(subtasks_scroll_key(), offset);
        }
        out.push(list.children(rows));
    }
    out.push(field(
        state,
        theme,
        &editor.new,
        Field::NewSubtask,
        "Add a subtask…",
    ));
    out
}

fn subtask_row(
    state: &AppState,
    theme: &Theme,
    task: &v1::Task,
    s: &v1::Subtask,
    editor: &SubtaskEditor,
) -> Node<Msg> {
    let link = TaskLink {
        task: task.id.clone(),
        subtask: Some(s.id.clone()),
    };
    let started = state.linked_thread(&link).is_some();
    let renaming = editor
        .renaming
        .as_ref()
        .filter(|(id, _)| *id == s.id)
        .map(|(_, f)| f);
    let title = match renaming {
        Some(f) => field(state, theme, f, Field::SubtaskTitle, "Subtask title"),
        None => Node::row()
            .width(Length::Fill(1.0))
            .height(Length::Fill(1.0))
            .key(key(&["subtask-title", &s.id]))
            .on_click(Msg::RenameSubtask(s.id.clone()))
            .cursor(urushi_ui::CursorIcon::Text)
            .role(Role::Button, format!("Rename subtask {}", s.title))
            .child(Node::text(
                &s.title,
                Font::Ui,
                theme.font_size,
                if s.done { theme.text_muted } else { theme.text },
            )),
    };
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(SUBTASK_ROW))
        .padding(Insets::symmetric(0.0, 4.0))
        .gap(8.0)
        .radius(theme.radius)
        .child(checkbox(
            theme,
            s.done,
            &s.title,
            Msg::ToggleSubtask(s.id.clone()),
        ))
        .child(title)
        .child(small_action(
            theme,
            if started {
                "Go to thread"
            } else {
                "Start thread"
            },
            if started {
                theme.accent
            } else {
                theme.text_muted
            },
            &format!(
                "{} for {}",
                if started {
                    "Go to thread"
                } else {
                    "Start a thread"
                },
                s.title
            ),
            Msg::StartThread(link),
        ))
        .child(icon_button(
            theme,
            "×",
            &format!("Delete subtask {}", s.title),
            Msg::DeleteSubtask(s.id.clone()),
        ))
}

fn draft_row(theme: &Theme, i: usize, title: &str) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(SUBTASK_ROW))
        .padding(Insets::symmetric(0.0, 4.0))
        .gap(8.0)
        .child(checkbox_shape(theme, false))
        .child(Node::text(title, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)))
        .child(icon_button(
            theme,
            "×",
            &format!("Remove subtask {title}"),
            Msg::RemoveDraftSubtask(i),
        ))
}

/// A check box that sends `on_click`.
fn checkbox(theme: &Theme, on: bool, what: &str, on_click: Msg) -> Node<Msg> {
    checkbox_shape(theme, on)
        .key(key(&["check", what]))
        .on_click(on_click)
        .role(
            Role::Switch,
            format!("{} {what}", if on { "Uncheck" } else { "Check" }),
        )
}

fn checkbox_shape(theme: &Theme, on: bool) -> Node<Msg> {
    let mut b = Node::row()
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .radius(4.0)
        .justify(Align::Center)
        .align(Align::Center)
        .border(1.5, if on { theme.accent } else { theme.text_muted })
        .background(if on { theme.accent } else { Color::TRANSPARENT });
    if on {
        b = b.child(Node::text(
            "✓",
            Font::UiSemibold,
            theme.font_size_small,
            theme.on_accent,
        ));
    }
    b
}

/// A small text button for a row (e.g. "Start thread").
pub(super) fn small_action(
    theme: &Theme,
    text: &str,
    color: Color,
    label: &str,
    on_click: Msg,
) -> Node<Msg> {
    Node::row()
        .padding(Insets::symmetric(2.0, 6.0))
        .radius(4.0)
        .hover_background(theme.hover)
        .key(key(&["small-action", label]))
        .on_click(on_click)
        .role(Role::Button, label)
        .child(Node::text(
            text,
            Font::UiMedium,
            theme.font_size_small,
            color,
        ))
}

/// Deleting a board takes its tasks with it, so it asks first. Only the red button
/// deletes (Enter doesn't).
pub(super) fn delete_board(theme: &Theme, name: &str, tasks: usize) -> Node<Msg> {
    let what = match tasks {
        0 => "It has no tasks.".to_owned(),
        1 => "Its 1 task will be deleted too.".to_owned(),
        n => format!("Its {n} tasks will be deleted too."),
    };
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(Node::text(what, Font::Ui, theme.font_size, theme.text))
        .child(hint(theme, "This can't be undone."));
    modal(
        theme,
        &format!("Delete board “{name}”?"),
        420.0,
        body,
        vec![
            cancel(theme),
            button(
                theme,
                "Delete board",
                ButtonKind::Danger,
                Msg::ConfirmDeleteBoard,
            ),
        ],
        Some(Msg::CloseDialog),
    )
}
