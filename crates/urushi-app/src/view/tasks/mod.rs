//! The task editor: title, notes, subtasks, column, kind, color and tags, and starting
//! a thread for the task or a subtask. On a Jira board (`jira.rs`) the issue's own
//! fields show read-only, and what Jira owns (deleting, nesting, attachments) is left
//! out.

use std::collections::HashMap;

use urushi_ui::widgets::{ButtonKind, button, modal};
use urushi_ui::{Align, Color, Font, Insets, Key, Length, Node, Role, TextField, Theme, key};

use super::dialogs::{area, cancel, field, hint, label};
use urushi_client::{FoundTask, TaskRef};

use crate::board::TaskLabels;
use crate::msg::{BoardMsg, Msg};
use crate::state::{AppState, Field, SubtaskEditor, TaskLink};

mod attachments;
mod jira;
mod labels;
mod nest;
mod subtasks;

pub(super) use nest::picker as nest_picker;
/// Lines of notes shown before the notes scroll.
const NOTES_LINES: u8 = 5;

/// Key of the editor's scrolling subtask list.
pub(crate) fn subtasks_scroll_key() -> Key {
    key(&["task-subtasks"])
}

/// Longest task title shown whole in the breadcrumb.
const CRUMB_CHARS: usize = 28;
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
    let found = task.and_then(|t| state.workspace.find_task(t));
    let current = found.as_ref().map(FoundTask::task);
    let issue = current.and_then(TaskRef::issue);
    let mut text = Node::column().width(Length::Fill(1.0)).gap(8.0);
    if let Some(found) = found.as_ref().filter(|f| f.path.len() > 1) {
        text = text.child(breadcrumb(theme, found));
    }
    text = text
        .child(label(theme, "Title"))
        .child(field(
            state,
            theme,
            title,
            Field::TaskTitle,
            "What needs doing?",
        ))
        .child(label(theme, "Notes"));
    text = text.children(notes_input(state, theme, notes, issue));
    if state.daemon_supports_subtasks() {
        let offset = scroll.get(&subtasks_scroll_key()).copied().unwrap_or(0.0);
        text = text.children(subtasks::checklist(state, theme, current, subtasks, offset));
    }
    let organize = organize(state, (theme, scroll), (board, column, current), labels);
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
    let actions = editor_actions(state, theme, found.as_ref(), task.is_some());
    modal(
        theme,
        match current {
            None => "New task",
            Some(c) if c.is_card() => "Edit task",
            Some(_) => "Edit subtask",
        },
        width,
        body,
        actions,
        Some(Msg::CloseDialog),
    )
}

/// The notes field; read-only for a Jira description too long to show whole (editing
/// it would cut it short in Jira).
fn notes_input(
    state: &AppState,
    theme: &Theme,
    notes: &TextField,
    issue: Option<&urushi_client::v1::JiraIssue>,
) -> Vec<Node<Msg>> {
    if issue.is_some_and(|i| !i.notes_editable) {
        return vec![
            Node::column()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(110.0))
                .padding(Insets::symmetric(6.0, 10.0))
                .radius(theme.radius)
                .background(theme.well)
                .clip()
                .child(urushi_ui::widgets::wrapped_text(
                    notes.text(),
                    Font::Ui,
                    theme.font_size,
                    theme.text_muted,
                )),
            hint(
                theme,
                "This description is too long to edit here; edit it in Jira.",
            ),
        ];
    }
    vec![area(
        state,
        theme,
        notes,
        Field::TaskNotes,
        ("Details, links… (optional)", NOTES_LINES),
    )]
}

/// The editor's organizing side: the kind (or a Jira issue's own fields), the column
/// (or a subtask's check mark), color and tags, and attachments (not on Jira boards).
fn organize(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    (board, column, current): (&str, &str, Option<TaskRef<'_>>),
    labels: &TaskLabels,
) -> Node<Msg> {
    let on_board = state.workspace.board(board);
    let jira_board = on_board.is_some_and(|b| b.jira.is_some());
    let issue = current.and_then(TaskRef::issue);
    let mut organize = Node::column().width(Length::Fill(1.0)).gap(8.0);
    if let (Some(i), Some(b), Some(c)) = (issue, on_board, current) {
        organize = organize.children(jira::issue_section(theme, b, i, c.kind()));
    } else if state.daemon_supports_jira() && current.is_none_or(TaskRef::is_card) {
        organize = organize.children(jira::kind_picker(
            theme,
            labels.kind,
            jira_board,
            side_width(state),
        ));
    }
    organize = match current.filter(|c| !c.is_card()) {
        Some(sub) => organize.children(nested_status(theme, sub, issue.is_none())),
        None => organize.child(label(theme, "Column")).child(column_chips(
            state,
            theme,
            board,
            column,
            side_width(state),
        )),
    };
    if state.daemon_supports_board_extras() {
        organize = organize.children(self::labels::section(
            state,
            theme,
            (board, side_width(state)),
            labels,
        ));
    }
    if state.daemon_supports_board_extras() && !jira_board {
        organize = organize.children(attachments::section(state, (theme, scroll), current));
    }
    organize
}

/// Where a subtask is: its card and the tasks between, each opening that task.
fn breadcrumb(theme: &Theme, found: &FoundTask<'_>) -> Node<Msg> {
    let mut row = Node::row().width(Length::Fill(1.0)).gap(4.0).clip();
    for (i, t) in found.path.iter().enumerate() {
        if i + 1 == found.path.len() {
            break;
        }
        row = row
            .child(
                Node::row()
                    .padding(Insets::symmetric(1.0, 4.0))
                    .radius(4.0)
                    .hover_background(theme.hover)
                    .key(key(&["breadcrumb", t.id()]))
                    .on_click(Msg::Board(BoardMsg::OpenTask(t.id().to_owned())))
                    .role(Role::Button, format!("Open {}", t.title()))
                    .child(Node::text(
                        super::board::shorten(t.title(), CRUMB_CHARS),
                        Font::UiMedium,
                        theme.font_size_small,
                        theme.accent,
                    )),
            )
            .child(Node::text(
                "›",
                Font::Ui,
                theme.font_size_small,
                theme.text_muted,
            ));
    }
    row
}

/// For a subtask, instead of the column: its check mark, and (unless Jira owns where
/// it is) taking it out onto the board.
fn nested_status(theme: &Theme, sub: TaskRef<'_>, movable: bool) -> Vec<Node<Msg>> {
    let done = sub.done();
    let standalone = movable.then(|| {
        button(
            theme,
            "Make standalone",
            ButtonKind::Ghost,
            Msg::Board(BoardMsg::MakeStandalone(sub.id().to_owned())),
        )
    });
    vec![
        label(theme, "Status"),
        Node::row()
            .gap(8.0)
            .align(Align::Center)
            .child(
                Node::row()
                    .gap(8.0)
                    .align(Align::Center)
                    .padding(Insets::symmetric(4.0, 6.0))
                    .radius(theme.radius)
                    .hover_background(theme.hover)
                    .key(key(&["subtask-done", sub.id()]))
                    .on_click(Msg::ToggleSubtask(sub.id().to_owned()))
                    .role(
                        Role::Switch,
                        if done { "Mark not done" } else { "Mark done" },
                    )
                    .child(subtasks::checkbox_shape(theme, done))
                    .child(Node::text(
                        if done { "Done" } else { "Not done" },
                        Font::UiMedium,
                        theme.font_size_small,
                        theme.text,
                    )),
            )
            .child(Node::spacer())
            .children(standalone),
    ]
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
    found: Option<&FoundTask<'_>>,
    editing: bool,
) -> Vec<Node<Msg>> {
    let mut actions = Vec::new();
    if let Some(found) = found {
        // Jira issues are deleted in Jira; the editor's Jira section opens it there.
        if found.task().issue().is_none() {
            let what = if found.task().is_card() {
                "Delete task"
            } else {
                "Delete subtask"
            };
            actions.push(button(theme, what, ButtonKind::Danger, Msg::DeleteTask));
        }
        actions.push(Node::spacer());
        if state.daemon_supports_subtasks() {
            let link = TaskLink::of(found);
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
