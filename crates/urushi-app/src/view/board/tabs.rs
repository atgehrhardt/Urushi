//! The board tabs across the top of the panel. A tab selects its board on click; on a
//! daemon that can reorder boards (1.17) it can be dragged onto another tab to take its
//! place, and its right-click menu moves it left or right. While a tab is dragged, the
//! dragged one is highlighted and an accent line shows where it would land, at the edge
//! of the tab whose place it takes. The line sits in a slot every tab always has, so
//! showing it never moves the tabs under the pointer.

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::{TAB_CHARS, jira, shorten};
use crate::msg::Msg;
use crate::organize::{Dragged, MenuTarget};
use crate::state::AppState;

/// Width of the slot at each end of a tab that shows the drop line.
const LINE_SLOT: f32 = 3.0;
/// Space between a tab's children (slots, badge, name).
const TAB_GAP: f32 = 6.0;

/// One tab per board, in the daemon's order.
pub(super) fn tabs<'a>(
    state: &'a AppState,
    theme: &'a Theme,
    shown: Option<&'a str>,
) -> impl Iterator<Item = Node<Msg>> + 'a {
    let boards = state.workspace.boards();
    let reorder = state.daemon_supports_board_order() && boards.len() > 1;
    let dragged = match &state.dragging {
        Some((Dragged::Board(b), _)) => Some(b.as_str()),
        _ => None,
    };
    boards.iter().map(move |b| {
        tab(
            state,
            theme,
            b,
            shown == Some(b.id.as_str()),
            reorder,
            dragged,
        )
    })
}

fn tab(
    state: &AppState,
    theme: &Theme,
    b: &v1::Board,
    active: bool,
    reorder: bool,
    dragged: Option<&str>,
) -> Node<Msg> {
    let (line_before, line_after) = drop_line_at(state, &b.id);
    let background = if active || dragged == Some(b.id.as_str()) {
        theme.selected
    } else {
        Color::TRANSPARENT
    };
    // Outer padding plus slot plus gap keeps the name 10 px from the tab's edge.
    let mut node = Node::row()
        .padding(Insets::symmetric(4.0, 10.0 - LINE_SLOT - TAB_GAP))
        .radius(theme.radius)
        .background(background)
        .hover_background(if active { theme.selected } else { theme.hover })
        .key(key(&["board-tab", &b.id]))
        .on_click(Msg::SelectBoard(b.id.clone()))
        .role(Role::Item, format!("Board {}", b.name))
        .gap(TAB_GAP)
        .align(Align::Center);
    if reorder {
        node = node
            .on_drag(Msg::DragBoard(b.id.clone()))
            .on_secondary_click(Msg::OpenMenu(MenuTarget::Board(b.id.clone())));
    }
    if dragged.is_some() {
        node = node
            .hover_background(theme.accent.with_alpha(0.15))
            .on_drop(Msg::DropBoard(b.id.clone()));
    }
    node.child(line_slot(theme, line_before))
        .children(jira::tab_badge(theme, b))
        .child(Node::text(
            shorten(&b.name, TAB_CHARS),
            Font::UiMedium,
            theme.font_size_small,
            if active { theme.text } else { theme.text_muted },
        ))
        .child(line_slot(theme, line_after))
}

/// The slot at one end of a tab, showing the drop line when `on`.
fn line_slot(theme: &Theme, on: bool) -> Node<Msg> {
    let slot = Node::new()
        .width(Length::Fixed(LINE_SLOT))
        .height(Length::Fixed(16.0))
        .radius(LINE_SLOT / 2.0);
    if on {
        slot.background(theme.accent)
    } else {
        slot
    }
}

/// While a board is dragged over `board`'s tab: whether the line showing where it would
/// land goes before this tab (moving left) or after it (moving right).
fn drop_line_at(state: &AppState, board: &str) -> (bool, bool) {
    let Some((Dragged::Board(dragged), _)) = &state.dragging else {
        return (false, false);
    };
    if state.board.tab_over.as_deref() != Some(board) {
        return (false, false);
    }
    let boards = state.workspace.boards();
    let at = |id: &str| boards.iter().position(|b| b.id == id);
    match (at(dragged), at(board)) {
        (Some(from), Some(to)) => (to < from, to > from),
        _ => (false, false),
    }
}
