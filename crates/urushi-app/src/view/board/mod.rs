//! The task-board panel: frosted glass that slides out from under the sidebar over the
//! terminals, blurring them. Board tabs across the top, then the board's columns of task
//! cards, all the same size. Cards open the task editor on click and move between
//! columns by drag and drop: while a card is dragged, its place stays as a faint outline, a line shows where
//! it would land, and a raised copy follows the pointer.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Color, Font, Insets, Key, Length, Node, Role, Theme, key};

use urushi_core::board::MAX_COLUMNS;

use super::labels;
use crate::board::{BOARDS_SINCE_MINOR, board_tags};
use crate::msg::Msg;
use crate::organize::{MenuTarget, Prompt};
use crate::state::AppState;

/// Space between cards in a column.
const CARD_GAP: f32 = 6.0;
/// Width of the card that follows the pointer while dragging.
const GHOST_WIDTH: f32 = 260.0;
/// Space above and below a card's contents.
const CARD_PADDING: f32 = 8.0;
/// Space between a card's title, notes and footer.
const CARD_ROW_GAP: f32 = 3.0;
/// Lines of notes a card shows.
const NOTES_LINES: f32 = 2.0;
/// Height of a card's footer row (subtask progress, thread chip).
const FOOTER_HEIGHT: f32 = 18.0;

mod card;
mod column;
mod dialogs;
pub(in crate::view) mod jira;

pub(in crate::view) use card::drag_ghost;
use column::column;
pub(in crate::view) use column::drop_line;
pub(in crate::view) use dialogs::{delete_board, delete_column};

/// Width of the slot that adds a column.
const ADD_COLUMN_WIDTH: f32 = 96.0;
/// Longest board name shown whole on its tab.
const TAB_CHARS: usize = 24;

/// `text`, cut to `max` characters with an ellipsis. For labels whose width comes from
/// their text (tabs, chips), which the layout doesn't shrink.
pub(in crate::view) fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

/// Key of a column's scrolling task list.
pub(crate) fn column_scroll_key(column: &str) -> Key {
    key(&["board-column", column])
}

/// The panel, slid out by `state.board.eased()`. Place it over the main area in a
/// clipped stack.
pub(super) fn panel(
    state: &AppState,
    theme: &Theme,
    scroll: &std::collections::HashMap<Key, f32>,
) -> Node<Msg> {
    let body = if state.daemon_minor < BOARDS_SINCE_MINOR {
        notice(
            theme,
            "Task boards need a newer daemon",
            "Restart the daemon from Settings to use them.",
            None,
        )
    } else if let Some(board) = state.board.board(&state.workspace) {
        columns(state, theme, board, scroll)
    } else {
        notice(
            theme,
            "No boards yet",
            "A board keeps tasks in columns: To do, In progress, Done.",
            Some(button(
                theme,
                "New board",
                ButtonKind::Primary,
                Msg::OpenPrompt(Prompt::NewBoard),
            )),
        )
    };
    let radius = if state.compact {
        0.0
    } else {
        theme.radius_large
    };
    urushi_ui::widgets::overlay(
        theme,
        Node::column()
            .width(Length::Fill(1.0))
            .height(Length::Fill(1.0)),
        radius,
    )
    .key(key(&["board-panel"]))
    .on_click(Msg::BoardBackground)
    .slide_x(1.0 - state.board.eased())
    .child(header(state, theme))
    .child(urushi_ui::widgets::divider(theme, false))
    .child(body)
}

/// Board tabs, "+", and the shown board's actions.
fn header(state: &AppState, theme: &Theme) -> Node<Msg> {
    let shown = state.board.board(&state.workspace).map(|b| b.id.clone());
    let compact = state.compact;
    let tabs = state.workspace.boards().iter().map(|b| {
        let active = shown.as_deref() == Some(b.id.as_str());
        Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .radius(theme.radius)
            .background(if active {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(if active { theme.selected } else { theme.hover })
            .key(key(&["board-tab", &b.id]))
            .on_click(Msg::SelectBoard(b.id.clone()))
            .role(Role::Item, format!("Board {}", b.name))
            .gap(6.0)
            .align(Align::Center)
            .children(jira::tab_badge(theme, b))
            .child(Node::text(
                shorten(&b.name, TAB_CHARS),
                Font::UiMedium,
                theme.font_size_small,
                if active { theme.text } else { theme.text_muted },
            ))
    });
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(44.0))
        .padding(Insets::symmetric(0.0, 12.0))
        .gap(4.0);
    // The panel covers the tab bar and its "Show sidebar" button, so it has its own.
    if !compact && state.sidebar.collapsed() {
        row = row.child(super::sidebar_toggle(state, theme));
    }
    // Phones need the room for the board tabs and actions.
    if !compact {
        row = row
            .child(Node::text(
                "Tasks",
                Font::UiSemibold,
                theme.font_size,
                theme.text,
            ))
            .child(Node::new().width(Length::Fixed(8.0)));
    }
    row = row.children(tabs);
    if state.daemon_supports_jira() {
        // A new board, or one imported from Jira.
        row = row.child(
            icon_button(theme, "+", "New board", Msg::OpenMenu(MenuTarget::NewBoard))
                .key(key(&["board-new"])),
        );
    } else if state.daemon_minor >= BOARDS_SINCE_MINOR {
        row = row.child(icon_button(
            theme,
            "+",
            "New board",
            Msg::OpenPrompt(Prompt::NewBoard),
        ));
    }
    row = row.child(Node::spacer());
    let board = state.board.board(&state.workspace);
    if let Some(filter) = board.and_then(|b| jira::epic_filter(state, theme, b)) {
        row = row.child(filter);
    }
    if let Some(filter) = board.and_then(|b| tag_filter(state, theme, b)) {
        row = row.child(filter);
    }
    match board {
        Some(b) if b.jira.is_some() => row = row.children(jira::actions(state, theme, b)),
        Some(b) => {
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
        None => {}
    }
    row.child(icon_button(
        theme,
        "×",
        "Close tasks (Esc)",
        Msg::ToggleBoard,
    ))
}

fn notice(theme: &Theme, title: &str, detail: &str, action: Option<Node<Msg>>) -> Node<Msg> {
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .justify(Align::Center)
        .align(Align::Center)
        .gap(8.0)
        .child(Node::text(
            title,
            Font::UiSemibold,
            theme.font_size + 2.0,
            theme.text,
        ))
        .child(Node::text(
            detail,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
        .children(action)
}

fn columns(
    state: &AppState,
    theme: &Theme,
    board: &v1::Board,
    scroll: &std::collections::HashMap<Key, f32>,
) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets::symmetric(14.0, 14.0))
        .gap(12.0)
        .align(Align::Stretch)
        .children(board.columns.iter().map(|c| {
            let offset = scroll
                .get(&column_scroll_key(&c.id))
                .copied()
                .unwrap_or(0.0);
            column(state, theme, board, c, offset)
        }))
        .children(
            (state.daemon_supports_board_extras()
                && board.jira.is_none()
                && board.columns.len() < MAX_COLUMNS)
                .then(|| add_column(theme, &board.id)),
        )
}

/// The narrow slot after the last column that adds one.
fn add_column(theme: &Theme, board: &str) -> Node<Msg> {
    Node::column()
        .width(Length::Fixed(ADD_COLUMN_WIDTH))
        .padding(Insets::symmetric(4.0, 0.0))
        .child(button(
            theme,
            "+ Column",
            ButtonKind::Ghost,
            Msg::OpenPrompt(Prompt::NewColumn(board.to_owned())),
        ))
}

/// The tag filter's button: "All tags" or the tag shown. Only on boards with tags.
fn tag_filter(state: &AppState, theme: &Theme, board: &v1::Board) -> Option<Node<Msg>> {
    if !state.daemon_supports_board_extras() || board_tags(board).is_empty() {
        return None;
    }
    let (text, color) = match &state.board.tag_filter {
        Some(tag) => (
            format!("#{}  ▾", shorten(tag, TAB_CHARS)),
            labels::tag_color(theme, tag),
        ),
        None => ("All tags  ▾".to_owned(), theme.text_muted),
    };
    Some(
        Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .radius(theme.radius)
            .border(1.0, theme.border)
            .hover_background(theme.hover)
            .key(key(&["board-tag-filter"]))
            .on_click(Msg::OpenMenu(MenuTarget::TagFilter(board.id.clone())))
            .role(Role::Button, "Filter by tag")
            .child(Node::text(
                text,
                Font::UiMedium,
                theme.font_size_small,
                color,
            )),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_labels_are_shortened_with_an_ellipsis() {
        assert_eq!(super::shorten("Sprint", 24), "Sprint");
        assert_eq!(super::shorten("abcdef ghij", 8), "abcdef…");
        assert_eq!(super::shorten("éééé", 3), "éé…");
    }
}
