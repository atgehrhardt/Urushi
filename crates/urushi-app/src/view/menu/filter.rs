//! The board's **Filters** menu: on a Jira board "Only my issues", "Recently updated"
//! and the board's quick filters, then a page per field with the values the board's
//! tasks have (several can be chosen), and "Clear filters".

use std::collections::HashMap;

use urushi_ui::{Key, Length, Node, Theme, key};

use super::super::labels::{color, tag_color};
use super::tasks::dot_item;
use super::{back, check, item, separator};
use crate::filter::{Dim, board_dims, board_values};
use crate::msg::{FilterMsg, Msg};
use crate::organize::MenuPage;
use crate::state::AppState;

/// Values shown before a page scrolls.
const VISIBLE: usize = 10;
/// Height of one menu row (as `item` draws it), with the gap under it.
const ROW: f32 = 29.0;

/// Key of a filter page's scrolling list.
pub(crate) fn filter_scroll_key() -> Key {
    key(&["filter-values"])
}

/// The menu's rows for `page`.
pub(super) fn filter_items(
    state: &AppState,
    theme: &Theme,
    (board, page): (&str, MenuPage),
    scroll: &HashMap<Key, f32>,
) -> Vec<Node<Msg>> {
    let Some(board) = state.workspace.board(board) else {
        return Vec::new();
    };
    match page {
        MenuPage::Filter(dim) => values_page(state, theme, board, dim, scroll),
        _ => main_page(state, theme, board),
    }
}

fn main_page(state: &AppState, theme: &Theme, board: &urushi_client::v1::Board) -> Vec<Node<Msg>> {
    let f = &state.board.filter;
    let mut items = Vec::new();
    if let Some(jira) = board.jira.as_deref() {
        if state.filter_context().me.is_some() {
            items.push(item(
                theme,
                "Only my issues",
                check(f.mine),
                theme.text,
                Msg::Filter(FilterMsg::Mine),
            ));
        }
        items.push(item(
            theme,
            "Recently updated",
            check(f.recent),
            theme.text,
            Msg::Filter(FilterMsg::Recent),
        ));
        for q in &jira.quick_filters {
            items.push(
                item(
                    theme,
                    &q.name,
                    check(f.quick.contains(&q.id)),
                    theme.text,
                    Msg::Filter(FilterMsg::Quick(q.id)),
                )
                .key(key(&["menu-quick", &q.id.to_string()])),
            );
        }
        items.push(separator(theme));
    }
    for dim in board_dims(board) {
        let chosen = f
            .chosen
            .get(&dim)
            .map_or(0, std::collections::BTreeSet::len);
        let trailing = if chosen > 0 {
            format!("{chosen}  ›")
        } else {
            "›".to_owned()
        };
        items.push(item(
            theme,
            dim.name(),
            &trailing,
            theme.text,
            Msg::MenuPage(MenuPage::Filter(dim)),
        ));
    }
    if f.active() {
        items.push(separator(theme));
        items.push(item(
            theme,
            "Clear filters",
            "",
            theme.text,
            Msg::Filter(FilterMsg::Clear),
        ));
    }
    items
}

fn values_page(
    state: &AppState,
    theme: &Theme,
    board: &urushi_client::v1::Board,
    dim: Dim,
    scroll: &HashMap<Key, f32>,
) -> Vec<Node<Msg>> {
    let f = &state.board.filter;
    let rows: Vec<Node<Msg>> = board_values(board, dim)
        .into_iter()
        .map(|(value, label)| {
            let fill = match dim {
                Dim::Label => Some(tag_color(theme, &label)),
                Dim::Epic => board
                    .epics
                    .iter()
                    .find(|e| e.key == value)
                    .and_then(|e| color(theme, e.color)),
                _ => None,
            };
            dot_item(
                theme,
                (&label, fill),
                f.has(dim, &value),
                key(&["menu-filter", dim.name(), &value]),
                Msg::Filter(FilterMsg::Toggle(dim, value.clone())),
            )
        })
        .collect();
    let mut list = Node::column().width(Length::Fill(1.0)).gap(1.0);
    if rows.len() > VISIBLE {
        #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
        let height = VISIBLE as f32 * ROW;
        let offset = scroll.get(&filter_scroll_key()).copied().unwrap_or(0.0);
        list = list
            .height(Length::Fixed(height))
            .scroll(filter_scroll_key(), offset);
    }
    vec![back(theme, dim.name()), list.children(rows)]
}
