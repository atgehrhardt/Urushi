//! A board tab's context menu: **Move left** and **Move right**, where the board can go
//! that way and the daemon can reorder boards (protocol 1.17).

use urushi_ui::{Node, Theme};

use super::item;
use crate::msg::{BoardMsg, Msg};
use crate::state::AppState;

pub(super) fn board_items(state: &AppState, theme: &Theme, id: &str) -> Vec<Node<Msg>> {
    let boards = state.workspace.boards();
    let Some(at) = boards.iter().position(|b| b.id == id) else {
        return Vec::new();
    };
    if !state.daemon_supports_board_order() {
        return Vec::new();
    }
    let moved = |index: usize| {
        Msg::Board(BoardMsg::MoveBoard {
            board: id.to_owned(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
        })
    };
    let mut items = Vec::new();
    if at > 0 {
        items.push(item(theme, "Move left", "", theme.text, moved(at - 1)));
    }
    if at + 1 < boards.len() {
        items.push(item(theme, "Move right", "", theme.text, moved(at + 1)));
    }
    items
}
