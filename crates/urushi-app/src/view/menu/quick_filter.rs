//! A quick filter chip's context menu (protocol 1.21): **Edit…** and **Delete…**.

use urushi_ui::{Node, Theme};

use super::item;
use crate::filter::quick::QuickRef;
use crate::msg::{FilterMsg, Msg};
use crate::state::AppState;

pub(super) fn quick_filter_items(
    state: &AppState,
    theme: &Theme,
    (board, filter): (&str, &QuickRef),
) -> Vec<Node<Msg>> {
    let known = state
        .workspace
        .board(board)
        .is_some_and(|b| filter.name(b).is_some());
    if !known || !state.daemon_supports_quick_filters() {
        return Vec::new();
    }
    vec![
        item(
            theme,
            "Edit…",
            "",
            theme.text,
            Msg::Filter(FilterMsg::OpenEdit(filter.clone())),
        ),
        item(
            theme,
            "Delete…",
            "",
            theme.danger,
            Msg::Filter(FilterMsg::OpenDelete(filter.clone())),
        ),
    ]
}
