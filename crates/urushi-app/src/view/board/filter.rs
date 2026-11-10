//! The board's filter bar, under its tabs, as Jira's: a search field, then (on a Jira
//! board, with room) the people issues are assigned to as avatars, "Only my issues",
//! "Recently updated" and the board's quick filters as chips, then **Filters ▾**
//! (everything, by field) and "Clear". On a phone: the search and **Filters ▾**.

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::jira::initials;
use super::shorten;
use crate::filter::{Dim, board_values};
use crate::msg::{FilterMsg, Msg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Field};

/// Most avatars shown; the rest are in the Assignee page of the menu.
const AVATARS: usize = 8;
/// Width of the search field (desktop).
const SEARCH_WIDTH: f32 = 220.0;
/// Longest quick filter name shown whole.
const CHIP_CHARS: usize = 22;

/// The bar for `board`.
pub(super) fn bar(state: &AppState, theme: &Theme, board: &v1::Board) -> Node<Msg> {
    let f = &state.board.filter;
    let search = super::super::dialogs::field(
        state,
        theme,
        &f.search,
        Field::BoardSearch,
        "Search this board",
    );
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(40.0))
        .padding(Insets::symmetric(0.0, 14.0))
        .gap(6.0)
        .align(Align::Center)
        .child(search.width(if state.compact {
            Length::Fill(1.0)
        } else {
            Length::Fixed(SEARCH_WIDTH)
        }));
    if let Some(jira) = board.jira.as_deref().filter(|_| !state.compact) {
        row = row.children(avatars(state, theme, board));
        if state.filter_context().me.is_some() {
            row = row.child(chip(
                theme,
                "Only my issues",
                f.mine,
                Msg::Filter(FilterMsg::Mine),
            ));
        }
        row = row.child(chip(
            theme,
            "Recently updated",
            f.recent,
            Msg::Filter(FilterMsg::Recent),
        ));
        for q in &jira.quick_filters {
            row = row.child(
                chip(
                    theme,
                    &shorten(&q.name, CHIP_CHARS),
                    f.quick.contains(&q.id),
                    Msg::Filter(FilterMsg::Quick(q.id)),
                )
                .key(key(&["quick-filter", &q.id.to_string()])),
            );
        }
    }
    if !state.compact {
        row = row.child(Node::spacer());
    }
    let count = f.count();
    let label = if count > 0 {
        format!("Filters ({count})  ▾")
    } else {
        "Filters  ▾".to_owned()
    };
    row = row.child(
        pill(theme, &label, count > 0)
            .key(key(&["board-filters"]))
            .on_click(Msg::OpenMenu(MenuTarget::Filter(board.id.clone())))
            .role(Role::Button, "Filters"),
    );
    if f.active() && !state.compact {
        row = row.child(urushi_ui::widgets::button(
            theme,
            "Clear",
            urushi_ui::widgets::ButtonKind::Ghost,
            Msg::Filter(FilterMsg::Clear),
        ));
    }
    row
}

/// The people the board's issues are assigned to, each toggling the filter on them.
fn avatars(state: &AppState, theme: &Theme, board: &v1::Board) -> Vec<Node<Msg>> {
    let side = 24.0;
    board_values(board, Dim::Assignee)
        .into_iter()
        .filter(|(id, _)| !id.is_empty())
        .take(AVATARS)
        .map(|(id, name)| {
            let on = state.board.filter.has(Dim::Assignee, &id);
            Node::row()
                .width(Length::Fixed(side))
                .height(Length::Fixed(side))
                .radius(side / 2.0)
                .justify(Align::Center)
                .align(Align::Center)
                .background(if on { theme.accent } else { theme.selected })
                .border(
                    if on { 2.0 } else { 1.0 },
                    if on { theme.text } else { theme.border },
                )
                .key(key(&["filter-avatar", &id]))
                .on_click(Msg::Filter(FilterMsg::Toggle(Dim::Assignee, id.clone())))
                .role(Role::Switch, format!("Assigned to {name}"))
                .child(Node::text(
                    initials(&name),
                    Font::UiSemibold,
                    9.0,
                    if on { theme.on_accent } else { theme.text },
                ))
        })
        .collect()
}

/// A filter that is on or off.
fn chip(theme: &Theme, label: &str, on: bool, msg: Msg) -> Node<Msg> {
    pill(theme, label, on)
        .on_click(msg)
        .role(Role::Switch, label)
}

fn pill(theme: &Theme, label: &str, on: bool) -> Node<Msg> {
    Node::row()
        .padding(Insets::symmetric(4.0, 10.0))
        .radius(theme.radius)
        .border(1.0, if on { theme.accent } else { theme.border })
        .background(if on {
            theme.accent.with_alpha(0.18)
        } else {
            Color::TRANSPARENT
        })
        .hover_background(theme.hover)
        .child(Node::text(
            label,
            Font::UiMedium,
            theme.font_size_small,
            if on { theme.text } else { theme.text_muted },
        ))
}
