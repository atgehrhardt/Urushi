//! A board tab's context menu: **Move left** and **Move right**, where the board can go
//! that way and the daemon can reorder boards (protocol 1.17). On a phone the same menu
//! opens from the board's "⋯" and starts with the board's actions, which its header has
//! no room for: Rename and Delete, or a Jira board's sync state, Sync now and Remove.
//! A phone's board picker lists the boards and the ways to add one.

use urushi_client::v1;
use urushi_ui::{Font, Insets, Length, Node, Theme};

use super::{check, item, jira, separator};
use crate::board::BOARDS_SINCE_MINOR;
use crate::msg::{BoardMsg, JiraMsg, Msg};
use crate::organize::Prompt;
use crate::state::AppState;
use crate::view::board::jira::sync_label;
use crate::view::board::shorten;

/// Longest board name shown whole in the picker.
const NAME_CHARS: usize = 28;
/// Longest sync state shown whole at the top of a Jira board's menu.
const STATUS_CHARS: usize = 34;

pub(super) fn board_items(state: &AppState, theme: &Theme, id: &str) -> Vec<Node<Msg>> {
    let boards = state.workspace.boards();
    let Some(at) = boards.iter().position(|b| b.id == id) else {
        return Vec::new();
    };
    let mut items = if state.compact {
        actions(state, theme, &boards[at])
    } else {
        Vec::new()
    };
    if !state.daemon_supports_board_order() {
        return items;
    }
    let moved = |index: usize| {
        Msg::Board(BoardMsg::MoveBoard {
            board: id.to_owned(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
        })
    };
    let mut order = Vec::new();
    if at > 0 {
        order.push(item(theme, "Move left", "", theme.text, moved(at - 1)));
    }
    if at + 1 < boards.len() {
        order.push(item(theme, "Move right", "", theme.text, moved(at + 1)));
    }
    if !items.is_empty() && !order.is_empty() {
        items.push(separator(theme));
    }
    items.extend(order);
    items
}

/// What a desktop's board header offers, as menu rows.
fn actions(state: &AppState, theme: &Theme, board: &v1::Board) -> Vec<Node<Msg>> {
    let Some(jira) = board.jira.as_deref() else {
        return vec![
            item(
                theme,
                "Rename…",
                "",
                theme.text,
                Msg::OpenPrompt(Prompt::RenameBoard(board.id.clone())),
            ),
            item(
                theme,
                "Delete…",
                "",
                theme.danger,
                Msg::OpenDeleteBoard(board.id.clone()),
            ),
        ];
    };
    let failed = !jira.error.is_empty();
    let status = Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets::symmetric(6.0, 10.0))
        .child(
            Node::text(
                shorten(&sync_label(jira, state.now_ms), STATUS_CHARS),
                Font::Ui,
                theme.font_size_small,
                if failed {
                    theme.danger
                } else {
                    theme.text_muted
                },
            )
            .width(Length::Fill(1.0)),
        );
    let sync = if state.jira.as_ref().is_some_and(|a| a.connected) {
        item(
            theme,
            "Sync now",
            "",
            theme.text,
            Msg::Jira(JiraMsg::Sync(board.id.clone())),
        )
    } else {
        item(
            theme,
            "Connect…",
            "",
            theme.text,
            Msg::Jira(JiraMsg::OpenConnect),
        )
    };
    vec![
        status,
        sync,
        item(
            theme,
            "Remove…",
            "",
            theme.danger,
            Msg::OpenDeleteBoard(board.id.clone()),
        ),
    ]
}

/// Every board, the shown one checked, then "New board…" (and "Import from Jira…").
pub(super) fn picker_items(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    let shown = state.board.board(&state.workspace).map(|b| b.id.as_str());
    let mut items: Vec<Node<Msg>> = state
        .workspace
        .boards()
        .iter()
        .map(|b| {
            item(
                theme,
                &shorten(&b.name, NAME_CHARS),
                check(shown == Some(b.id.as_str())),
                theme.text,
                Msg::SelectBoard(b.id.clone()),
            )
        })
        .collect();
    if state.daemon_minor < BOARDS_SINCE_MINOR {
        return items;
    }
    if !items.is_empty() {
        items.push(separator(theme));
    }
    if state.daemon_supports_jira() {
        items.extend(jira::new_board_items(theme));
    } else {
        items.push(item(
            theme,
            "New board…",
            "",
            theme.text,
            Msg::OpenPrompt(Prompt::NewBoard),
        ));
    }
    items
}
