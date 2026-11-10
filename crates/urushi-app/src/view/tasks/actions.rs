//! The task editor's buttons: Delete, Start thread, Cancel and Save; a new subtask's
//! Remove, Done, Cancel and Save; and the question before unsaved edits are dropped.

use urushi_client::FoundTask;
use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Font, Node, Theme};

use super::super::dialogs::cancel;
use crate::msg::{BoardMsg, Msg};
use crate::state::{AppState, TaskLink};

/// Closing with unsaved edits: discard them, or keep editing.
pub(super) fn discard_question(theme: &Theme) -> Vec<Node<Msg>> {
    vec![
        Node::text(
            "Discard unsaved changes?",
            Font::UiMedium,
            theme.font_size,
            theme.text,
        ),
        Node::spacer(),
        button(
            theme,
            "Keep editing",
            ButtonKind::Secondary,
            Msg::Board(BoardMsg::KeepEditing),
        ),
        button(
            theme,
            "Discard",
            ButtonKind::Danger,
            Msg::Board(BoardMsg::DiscardEdits),
        ),
    ]
}

/// A new subtask's editor: Remove it, Done (back to its task, keeping it), Cancel
/// (everything), and Save (its task, with it).
pub(super) fn draft_actions(theme: &Theme) -> Vec<Node<Msg>> {
    vec![
        button(
            theme,
            "Remove subtask",
            ButtonKind::Danger,
            Msg::Board(BoardMsg::RemoveSubtaskDraft),
        ),
        Node::spacer(),
        button(
            theme,
            "Done",
            ButtonKind::Secondary,
            Msg::Board(BoardMsg::EditorBack),
        ),
        cancel(theme),
        button(theme, "Save", ButtonKind::Primary, Msg::Submit),
    ]
}

/// Delete and Start thread (for an existing task), Cancel, and Save or Add task.
pub(super) fn editor_actions(
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
