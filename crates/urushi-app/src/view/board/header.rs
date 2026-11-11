//! The board panel's header on a desktop or a foldable: "Tasks", the board tabs and "+"
//! (scrolling sideways when there are more than fit), the shown board's actions, and ×.
//! A panel too narrow for the actions (an unfolded foldable, a narrow window, a wide
//! sidebar) has them behind "⋯", like a phone (`phone.rs`); see [`narrow`].

use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Font, Insets, Key, Length, Node, Theme, key};

use super::{jira, tabs};
use crate::board::BOARDS_SINCE_MINOR;
use crate::msg::Msg;
use crate::organize::{MenuTarget, Prompt};
use crate::state::AppState;

/// Below this width (logical pixels) the board panel is narrow: the header's actions go
/// behind "⋯" and the filter bar keeps only the search and **Filters ▾**.
const NARROW_PANEL: f32 = 880.0;
/// Width of the sidebar's resize edge, beside it.
const SIDEBAR_EDGE: f32 = super::super::EDGE;

/// Whether the board panel is too narrow for a desktop's header and filter bar: on a
/// phone, or when the window less the sidebar is under [`NARROW_PANEL`]. Before the
/// window's width is known, it isn't.
pub(in crate::view) fn narrow(state: &AppState) -> bool {
    if state.compact {
        return true;
    }
    let window = state.sidebar.window_width();
    if window <= 0.0 {
        return false;
    }
    let sidebar = (state.sidebar.width() + SIDEBAR_EDGE) * state.sidebar.shown();
    window - sidebar - 2.0 * super::super::PANEL_MARGIN < NARROW_PANEL
}

/// Board tabs, "+", the shown board's actions (or "⋯" when [`narrow`]) and ×.
pub(super) fn header(
    state: &AppState,
    theme: &Theme,
    scroll: &std::collections::HashMap<Key, f32>,
) -> Node<Msg> {
    let narrow = narrow(state);
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(44.0))
        .padding(Insets::symmetric(0.0, 12.0))
        .gap(4.0)
        .align(Align::Center);
    // The panel covers the tab bar and its "Show sidebar" button, so it has its own.
    if state.sidebar.collapsed() {
        row = row.child(super::super::sidebar_toggle(state, theme));
    }
    if !narrow {
        row = row
            .child(Node::text(
                "Tasks",
                Font::UiSemibold,
                theme.font_size,
                theme.text,
            ))
            .child(Node::new().width(Length::Fixed(8.0)));
    }
    row = row.child(tab_strip(state, theme, scroll));
    let Some(b) = state.board.board(&state.workspace) else {
        return row.child(close(theme));
    };
    if narrow {
        row = row.child(
            icon_button(
                theme,
                "⋯",
                "Board actions",
                Msg::OpenMenu(MenuTarget::Board(b.id.clone())),
            )
            .key(key(&["board-actions"])),
        );
    } else if b.jira.is_some() {
        row = row.children(jira::actions(state, theme, b));
    } else {
        row = row
            .child(button(
                theme,
                "Rename",
                ButtonKind::Ghost,
                Msg::OpenPrompt(Prompt::RenameBoard(b.id.clone())),
            ))
            .child(button(
                theme,
                "Delete",
                ButtonKind::Ghost,
                Msg::OpenDeleteBoard(b.id.clone()),
            ));
    }
    row.child(close(theme))
}

/// The board tabs and "+", taking the room the actions leave and scrolling sideways
/// when they need more.
fn tab_strip(
    state: &AppState,
    theme: &Theme,
    scroll: &std::collections::HashMap<Key, f32>,
) -> Node<Msg> {
    let shown = state.board.board(&state.workspace).map(|b| b.id.as_str());
    let strip_key = key(&["board-tabs"]);
    let offset = scroll.get(&strip_key).copied().unwrap_or(0.0);
    let mut strip = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .gap(4.0)
        .align(Align::Center)
        .scroll_x(strip_key, offset)
        .children(tabs::tabs(state, theme, shown));
    if state.daemon_supports_jira() {
        // A new board, or one imported from Jira.
        strip = strip.child(
            icon_button(theme, "+", "New board", Msg::OpenMenu(MenuTarget::NewBoard))
                .key(key(&["board-new"])),
        );
    } else if state.daemon_minor >= BOARDS_SINCE_MINOR {
        strip = strip.child(icon_button(
            theme,
            "+",
            "New board",
            Msg::OpenPrompt(Prompt::NewBoard),
        ));
    }
    strip
}

fn close(theme: &Theme) -> Node<Msg> {
    icon_button(theme, "×", "Close tasks (Esc)", Msg::ToggleBoard)
}
