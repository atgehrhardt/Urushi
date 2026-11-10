//! The board's filter bar, under its tabs, as Jira's: a search field, then (with room)
//! on a Jira board the people issues are assigned to as avatars, "Only my issues",
//! "Recently updated" and Jira's quick filters as chips, and on any board the quick
//! filters it keeps as chips (right-click one to change or delete it), then
//! **Filters ▾** (everything, by field), "Clear" and "Save filter…". On a phone: the
//! search and **Filters ▾**.

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::jira::initials;
use super::shorten;
use crate::filter::quick::QuickRef;
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
            row = row.child(quick_chip(
                state,
                theme,
                (board, &q.name, QuickRef::Jira(q.id)),
                Msg::Filter(FilterMsg::Quick(q.id)),
            ));
        }
    }
    if !state.compact {
        for q in &board.quick_filters {
            row = row.child(quick_chip(
                state,
                theme,
                (board, &q.name, QuickRef::Kept(q.id.clone())),
                Msg::Filter(FilterMsg::Saved(q.id.clone())),
            ));
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
    if !state.compact && state.can_save_quick_filter(board) {
        row = row.child(
            urushi_ui::widgets::button(
                theme,
                "Save filter…",
                urushi_ui::widgets::ButtonKind::Ghost,
                Msg::Filter(FilterMsg::OpenSave),
            )
            .key(key(&["save-quick-filter"])),
        );
    }
    row
}

/// A quick filter's chip: a click turns it on or off, a right-click (on a daemon that
/// can change quick filters) opens its menu.
fn quick_chip(
    state: &AppState,
    theme: &Theme,
    (board, name, filter): (&v1::Board, &str, QuickRef),
    toggle: Msg,
) -> Node<Msg> {
    let id = match &filter {
        QuickRef::Jira(id) => id.to_string(),
        QuickRef::Kept(id) => id.clone(),
    };
    let mut node = chip(
        theme,
        &shorten(name, CHIP_CHARS),
        filter.is_on(&state.board.filter),
        toggle,
    )
    .key(key(&["quick-filter", &id]));
    if state.daemon_supports_quick_filters() {
        node = node.on_secondary_click(Msg::OpenMenu(MenuTarget::QuickFilter(
            board.id.clone(),
            filter,
        )));
    }
    node
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
