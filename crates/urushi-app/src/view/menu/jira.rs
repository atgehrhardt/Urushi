//! Menus for Jira boards (protocol 1.12): the panel's "+" (a new board, or one
//! imported from Jira) and a Jira board's epic filter.

use urushi_ui::{Node, Theme, key};

use super::super::labels::color;
use super::tasks::dot_item;
use super::{check, item};
use crate::msg::{BoardMsg, JiraMsg, Msg};
use crate::organize::Prompt;
use crate::state::AppState;

/// "New board…" and "Import from Jira…".
pub(super) fn new_board_items(theme: &Theme) -> Vec<Node<Msg>> {
    vec![
        item(
            theme,
            "New board…",
            "",
            theme.text,
            Msg::OpenPrompt(Prompt::NewBoard),
        ),
        item(
            theme,
            "Import from Jira…",
            "",
            theme.text,
            Msg::Jira(JiraMsg::OpenImport),
        ),
    ]
}

/// "All tasks", then one row per epic the board's cards are in.
pub(super) fn epic_filter_items(state: &AppState, theme: &Theme, board: &str) -> Vec<Node<Msg>> {
    let Some(board) = state.workspace.board(board) else {
        return Vec::new();
    };
    let current = state.board.epic_filter.as_deref();
    let mut items = vec![item(
        theme,
        "All tasks",
        check(current.is_none()),
        theme.text,
        Msg::Board(BoardMsg::FilterEpic(None)),
    )];
    items.extend(crate::board::board_epics(board).into_iter().map(|e| {
        dot_item(
            theme,
            (&e.summary, color(theme, e.color)),
            current == Some(e.key.as_str()),
            key(&["menu-epic", &e.key]),
            Msg::Board(BoardMsg::FilterEpic(Some(e.key.clone()))),
        )
    }));
    items
}
