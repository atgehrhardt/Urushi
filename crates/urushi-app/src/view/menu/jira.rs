//! Menus for Jira boards (protocol 1.12): the panel's "+" (a new board, or one
//! imported from Jira).

use urushi_ui::{Node, Theme};

use super::item;
use crate::msg::{JiraMsg, Msg};
use crate::organize::Prompt;

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
