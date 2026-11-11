//! The task-board panel: frosted glass that slides out from under the sidebar over the
//! terminals, blurring them. Board tabs across the top (`tabs.rs`), then the board's columns of task
//! cards, all the same size. Columns share the width but never get narrower than
//! [`BOARD_COLUMN_MIN_WIDTH`]: past that the board scrolls sideways (a foldable, a narrow
//! window, many columns). A phone shows one column at a time instead (`phone.rs`). Cards open the task editor on click and move between
//! columns by drag and drop: while a card is dragged, its place stays as a faint outline, a line shows where
//! it would land, and a raised copy follows the pointer.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Align, Font, Insets, Key, Length, Node, Theme, key};

use urushi_core::board::MAX_COLUMNS;

use crate::board::BOARDS_SINCE_MINOR;
use crate::msg::Msg;
use crate::organize::Prompt;
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
mod filter;
mod header;
pub(in crate::view) mod jira;
mod phone;
pub(in crate::view) mod quick_filter;
mod tabs;

pub(in crate::view) use card::drag_ghost;
use column::column;
pub(in crate::view) use column::drop_line;
pub(in crate::view) use dialogs::{delete_board, delete_column, delete_issue};
pub(in crate::view) use header::narrow;

/// The narrowest a column gets beside others, so its cards stay readable: the width of
/// the card that follows the pointer while dragging, plus the column's padding.
pub(crate) const BOARD_COLUMN_MIN_WIDTH: f32 = GHOST_WIDTH + 20.0;

/// Key of a board's sideways scrolling row of columns.
fn board_columns_scroll_key(board: &str) -> Key {
    key(&["board-columns", board])
}

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
        if state.compact {
            phone::body(state, theme, board, scroll)
        } else {
            columns(state, theme, board, scroll)
        }
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
    let filter_bar = state
        .board
        .board(&state.workspace)
        .filter(|_| state.daemon_supports_board_extras())
        .map(|b| filter::bar(state, theme, b, scroll));
    let column_tabs = state
        .board
        .board(&state.workspace)
        .filter(|_| state.compact && state.daemon_minor >= BOARDS_SINCE_MINOR)
        .map(|b| phone::column_tabs(state, theme, b));
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
    .child(if state.compact {
        phone::header(state, theme)
    } else {
        header::header(state, theme, scroll)
    })
    .children(filter_bar)
    .children(column_tabs)
    .child(urushi_ui::widgets::divider(theme, false))
    .child(body)
    .children(super::error::for_place(
        state,
        theme,
        crate::errors::ErrorPlace::Board,
        None,
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
    let key = board_columns_scroll_key(&board.id);
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets::symmetric(14.0, 14.0))
        .gap(12.0)
        .align(Align::Stretch)
        .scroll_x(key, scroll.get(&key).copied().unwrap_or(0.0))
        .children(board.columns.iter().map(|c| {
            let offset = scroll
                .get(&column_scroll_key(&c.id))
                .copied()
                .unwrap_or(0.0);
            column(state, theme, board, c, offset).min_width(BOARD_COLUMN_MIN_WIDTH)
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

#[cfg(test)]
mod tests {
    #[test]
    fn long_labels_are_shortened_with_an_ellipsis() {
        assert_eq!(super::shorten("Sprint", 24), "Sprint");
        assert_eq!(super::shorten("abcdef ghij", 8), "abcdef…");
        assert_eq!(super::shorten("éééé", 3), "éé…");
    }
}
