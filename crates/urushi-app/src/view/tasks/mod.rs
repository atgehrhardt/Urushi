//! The task editor: title, notes, subtasks, column, kind, color and tags, and starting
//! a thread for the task or a subtask. On a Jira board (`jira.rs`) the issue's own
//! fields show read-only, and what Jira owns (deleting, nesting, attachments) is left
//! out.

use std::collections::HashMap;

use urushi_ui::widgets::{ButtonKind, button, modal_with_header};
use urushi_ui::{Align, Color, Font, Insets, Key, Length, Node, Role, TextField, Theme, key};

use super::dialogs::{area, field, hint, label};
use urushi_client::{FoundTask, TaskRef, v1};

use crate::board::TaskLabels;
use crate::issue::IssueEditor;
use crate::msg::{BoardMsg, Msg};
use crate::state::{AppState, EditorNav, Field, SubtaskEditor};

mod actions;
mod attachments;
mod header;
mod issue;
mod jira;
mod labels;
mod nest;
mod pull_request;
mod subtasks;

pub(super) use nest::picker as nest_picker;
/// Lines of notes shown before the notes scroll.
const NOTES_LINES: u8 = 5;

/// Key of the editor's scrolling subtask list.
pub(crate) fn subtasks_scroll_key() -> Key {
    key(&["task-subtasks"])
}

/// Width of the editor with its two columns (desktop).
const WIDE: f32 = 780.0;
/// Width of the editor of a Jira issue in full (desktop).
const FULL: f32 = 960.0;
/// Width of the one-column editor (phones).
const NARROW: f32 = 520.0;
/// Height of the one-column editor's scrolling body (phones).
const NARROW_BODY: f32 = 560.0;

/// Key of the one-column editor's scrolling body.
pub(crate) fn editor_scroll_key() -> Key {
    key(&["task-editor"])
}

/// The editor's subtasks (none in a new subtask's own editor). An epic on a Jira board
/// (the task's kind, or the one picked for a new task) holds stories, not sub-tasks
/// (ADR 0027), so it is offered none.
fn subtask_list(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    (board, current, draft, picked): (&str, Option<TaskRef<'_>>, bool, i32),
    subtasks: &SubtaskEditor,
) -> Vec<Node<Msg>> {
    if !state.daemon_supports_subtasks() || draft {
        return Vec::new();
    }
    let offset = scroll.get(&subtasks_scroll_key()).copied().unwrap_or(0.0);
    let epic = state
        .workspace
        .board(board)
        .is_some_and(|b| b.jira.is_some())
        && current.map_or(picked, TaskRef::kind) == i32::from(v1::TaskKind::Epic);
    subtasks::checklist(state, theme, (current, epic), subtasks, offset)
}

/// Creating (`task: None`) or editing a task. On a desktop the text (title, notes,
/// subtasks) is on the left and the organizing (column, color, tags) on the right; on a
/// phone they are one scrolling column.
pub(super) fn editor(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    (board, column, task, nav): (&str, &str, Option<&str>, &EditorNav),
    (title, notes, subtasks, labels): (&TextField, &TextField, &SubtaskEditor, &TaskLabels),
    jira: &IssueEditor,
) -> Node<Msg> {
    let found = task.and_then(|t| state.workspace.find_task(t));
    let current = found.as_ref().map(FoundTask::task);
    let issue = current.and_then(TaskRef::issue);
    // A Jira issue in full gets more room, and scrolls.
    let full = jira.details.is_some() || jira.loading;
    // A new subtask's editor (ADR 0025): it has no subtasks or column of its own.
    let draft = nav.draft_of_previous.is_some();
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
        .child(label(theme, "Notes"));
    text = text.children(notes_input(state, theme, notes, issue));
    let list = (board, current, draft, labels.kind);
    text = text.children(subtask_list(state, (theme, scroll), list, subtasks));
    if let (Some(task), true) = (task, issue.is_some() && full) {
        let width = text_width(state, full);
        text = text.children(issue::tabs(state, theme, (task, width), jira, scroll));
    }
    let organize = organize(
        state,
        (theme, scroll),
        (board, column, current, draft),
        labels,
        jira,
    );
    let hint_text = if draft {
        format!(
            "Back keeps this subtask in its task's list. Save ({}) saves the task with it.",
            save_keys()
        )
    } else if state.daemon_supports_subtasks() {
        format!(
            "Enter adds a subtask, a tag or a notes line. Nothing is saved until Save ({}).",
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
        let offset = scroll.get(&editor_scroll_key()).copied().unwrap_or(0.0);
        let mut body = Node::column()
            .width(Length::Fill(1.0))
            .gap(12.0)
            .child(columns)
            .child(hint(theme, &hint_text))
            .scroll(editor_scroll_key(), offset);
        // A Jira issue in full keeps one height while it loads; any other task's
        // editor is as tall as it needs, up to what keeps its buttons on screen.
        body = if full {
            body.height(Length::Fixed(full_height(state)))
        } else {
            body.max_height(full_height(state))
        };
        (body, if full { FULL } else { WIDE })
    };
    let actions = if nav.confirm_discard {
        actions::discard_question(theme)
    } else if draft {
        actions::draft_actions(theme)
    } else {
        actions::editor_actions(state, theme, found.as_ref(), task.is_some())
    };
    let (header, name) = header::header(state, theme, found.as_ref(), nav);
    modal_with_header(
        theme,
        (header, &name),
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
/// (or a subtask's check mark), color and tags, and a pull request link and attachments
/// (not on Jira boards).
fn organize(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    (board, column, current, draft): (&str, &str, Option<TaskRef<'_>>, bool),
    labels: &TaskLabels,
    jira: &IssueEditor,
) -> Node<Msg> {
    let on_board = state.workspace.board(board);
    let jira_board = on_board.is_some_and(|b| b.jira.is_some());
    let issue = current.and_then(TaskRef::issue);
    let mut organize = Node::column().width(Length::Fill(1.0)).gap(8.0);
    let width = side_width(state);
    if let (Some(i), Some(_), true) = (issue, on_board, jira.details.is_some()) {
        organize = organize.children(issue::section(state, theme, (i, width), jira, scroll));
    } else if let (Some(i), Some(b), Some(c)) = (issue, on_board, current) {
        organize = organize.children(jira::issue_section(theme, b, i, c.kind()));
    } else if state.daemon_supports_jira() && current.is_none_or(TaskRef::is_card) {
        // A Jira board's subtasks are all Sub-task issues: no kind to pick.
        if !(draft && jira_board) {
            let kinds = (jira_board, state.daemon_supports_epic_cards());
            organize = organize.children(jira::kind_picker(theme, labels.kind, kinds, width));
        }
        if jira_board && current.is_none() {
            organize = organize.children(issue::create_section(state, theme, jira, width, scroll));
        }
    }
    organize = match current.filter(|c| !c.is_card()) {
        Some(sub) => organize.children(nested_status(theme, sub, issue.is_none())),
        // A new subtask goes under its task, not in a column.
        None if draft => organize,
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
    if state.shows_pr_field() {
        organize = organize.children(pull_request::section(state, theme, labels));
    }
    if state.daemon_supports_board_extras() && !jira_board {
        organize = organize.children(attachments::section(state, (theme, scroll), current));
    }
    organize
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

/// The width of the editor's left-hand column (the whole body on phones), for wrapping
/// comments.
fn text_width(state: &AppState, full: bool) -> f32 {
    if state.compact {
        NARROW - 40.0
    } else {
        ((if full { FULL } else { WIDE }) - 40.0 - 24.0) * 1.4 / 2.4
    }
}

/// The height of a Jira issue's scrolling editor body: as much of the window as leaves
/// room for its top bar and buttons.
fn full_height(state: &AppState) -> f32 {
    if state.window_height <= 0.0 {
        640.0
    } else {
        (state.window_height - 200.0).clamp(360.0, 780.0)
    }
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
