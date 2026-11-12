//! The board's filter bar, under its tabs, as Jira's: a search field, then (with room)
//! on a Jira board the people issues are assigned to as avatars, "Only my issues",
//! "Recently updated" and Jira's quick filters as chips, and on any board the quick
//! filters it keeps as chips (right-click one to change or delete it), then
//! **Filters ▾** (everything, by field), "Clear" and "Save filter…". The chips scroll
//! sideways when they don't fit. On a phone, or a panel too narrow for them
//! (`narrow`): the search and **Filters ▾**.

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::jira::initials;
use super::shorten;
use crate::filter::Dim;
use crate::filter::quick::QuickRef;
use crate::msg::{FilterMsg, Msg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Field};

/// Most avatars shown; the rest are in the Assignee page of the menu.
const AVATARS: usize = 8;
/// Width of the search field (desktop).
const SEARCH_WIDTH: f32 = 220.0;
/// Longest quick filter name shown whole.
const CHIP_CHARS: usize = 22;

/// The bar for `board`. `scroll` holds the offset of its sideways scrolling chips.
pub(super) fn bar(
    state: &AppState,
    theme: &Theme,
    board: &v1::Board,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Node<Msg> {
    let f = &state.board.filter;
    let search = super::super::dialogs::field(
        state,
        theme,
        &f.search,
        Field::BoardSearch,
        "Search this board",
    );
    let narrow = super::narrow(state);
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(40.0))
        .padding(Insets::symmetric(0.0, 14.0))
        .gap(6.0)
        .align(Align::Center)
        .child(search.width(if narrow {
            Length::Fill(1.0)
        } else {
            Length::Fixed(SEARCH_WIDTH)
        }));
    // With room, the chips share what the search and the buttons leave, and scroll
    // sideways when they need more.
    let mut chips = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center);
    if let Some(jira) = board.jira.as_deref().filter(|_| !narrow) {
        chips = chips.children(avatars(state, theme, board));
        if state.filter_context().me.is_some() {
            chips = chips.child(chip(
                theme,
                "Only my issues",
                f.mine,
                Msg::Filter(FilterMsg::Mine),
            ));
        }
        chips = chips.child(chip(
            theme,
            "Recently updated",
            f.recent,
            Msg::Filter(FilterMsg::Recent),
        ));
        for q in &jira.quick_filters {
            chips = chips.child(quick_chip(
                state,
                theme,
                (board, &q.name, QuickRef::Jira(q.id)),
                Msg::Filter(FilterMsg::Quick(q.id)),
            ));
        }
    }
    if !narrow {
        for q in &board.quick_filters {
            chips = chips.child(quick_chip(
                state,
                theme,
                (board, &q.name, QuickRef::Kept(q.id.clone())),
                Msg::Filter(FilterMsg::Saved(q.id.clone())),
            ));
        }
    }
    if !narrow {
        let chips_key = key(&["board-filter-chips", &board.id]);
        let offset = scroll.get(&chips_key).copied().unwrap_or(0.0);
        row = row.child(chips.scroll_x(chips_key, offset));
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
    if f.active() && !narrow {
        row = row.child(urushi_ui::widgets::button(
            theme,
            "Clear",
            urushi_ui::widgets::ButtonKind::Ghost,
            Msg::Filter(FilterMsg::Clear),
        ));
    }
    if !narrow && state.can_save_quick_filter(board) {
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
    let people = state.board.filtered.values(board, Dim::Assignee);
    avatar_people(&people, &state.board.filter)
        .into_iter()
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

/// Who gets an avatar: the first [`AVATARS`] people by name, then anyone else chosen
/// (in the menu), so every person filtered on shows and avatars don't move as issues
/// do.
fn avatar_people(
    people: &[(String, String)],
    filter: &crate::filter::BoardFilter,
) -> Vec<(String, String)> {
    let mut people: Vec<(String, String)> = people
        .iter()
        .filter(|(id, _)| !id.is_empty())
        .cloned()
        .collect();
    people.sort_by_cached_key(|(id, name)| (name.to_lowercase(), id.clone()));
    let mut shown = 0;
    people.retain(|(id, _)| {
        shown += 1;
        shown <= AVATARS || filter.has(Dim::Assignee, id)
    });
    people
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
        // A chip that is on stays the accent's color under the pointer, a little
        // stronger, so it never looks off while hovered.
        .hover_background(if on {
            theme.accent.with_alpha(0.28)
        } else {
            theme.hover
        })
        .child(Node::text(
            label,
            Font::UiMedium,
            theme.font_size_small,
            if on { theme.text } else { theme.text_muted },
        ))
}

#[cfg(test)]
mod tests {
    use urushi_ui::{Point, Primitive, Rect, TextSystem, Theme, build};

    use super::{AVATARS, avatar_people, chip};
    use crate::filter::{BoardFilter, Dim};
    use crate::msg::Msg;

    #[test]
    fn a_chip_that_is_on_keeps_its_color_under_the_pointer() {
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        let mut fill = |on: bool| {
            let frame = build(
                &chip(&theme, "Bugs", on, Msg::CloseMenu),
                Rect::new(0.0, 0.0, 100.0, 30.0),
                &mut text,
                Some(Point::new(5.0, 5.0)),
            );
            frame.scene.primitives.iter().find_map(|p| match p {
                Primitive::Quad(q) => Some(q.color),
                _ => None,
            })
        };
        assert_eq!(fill(true), Some(theme.accent.with_alpha(0.28)));
        assert_eq!(fill(false), Some(theme.hover));
    }

    #[test]
    fn avatars_go_by_name_and_everyone_chosen_has_one() {
        let people: Vec<(String, String)> = (0..12)
            .rev()
            .map(|n| (format!("acc-{n}"), format!("Person {n:02}")))
            .chain([(String::new(), "Unassigned".to_owned())])
            .collect();
        let mut f = BoardFilter::default();
        let shown = avatar_people(&people, &f);
        assert_eq!(shown.len(), AVATARS);
        assert_eq!(shown[0].1, "Person 00");
        f.toggle(Dim::Assignee, "acc-11");
        f.toggle(Dim::Assignee, "acc-0");
        let shown = avatar_people(&people, &f);
        assert_eq!(shown.len(), AVATARS + 1);
        assert_eq!(shown[0].1, "Person 00", "a chosen one keeps its place");
        assert_eq!(shown[AVATARS].0, "acc-11", "one past the first is added");
    }
}
