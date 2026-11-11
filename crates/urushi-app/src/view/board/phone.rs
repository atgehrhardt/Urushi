//! The board on a phone (ADR 0046): one column at a time, at full width. The header holds
//! a board picker ("Name ▾"), the board's actions behind "⋯", and ×, so all of it fits.
//! Under the filter bar a tab per column, with its count, picks the column shown (a
//! sideways swipe steps through them too). While a card is dragged, the tabs take it:
//! dropped on one, it goes to the bottom of that column.

use urushi_client::v1;
use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use urushi_core::board::MAX_COLUMNS;

use super::{TAB_CHARS, column_scroll_key, jira, shorten};
use crate::board::BOARDS_SINCE_MINOR;
use crate::msg::Msg;
use crate::organize::{MenuTarget, Prompt};
use crate::state::AppState;

/// Height of the header and of the column tabs.
const ROW_HEIGHT: f32 = 44.0;
/// Height of the line under the shown column's tab.
const UNDERLINE: f32 = 2.0;

/// The board picker, the board's actions and ×.
pub(super) fn header(state: &AppState, theme: &Theme) -> Node<Msg> {
    let board = state.board.board(&state.workspace);
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(ROW_HEIGHT))
        .padding(Insets::symmetric(0.0, 12.0))
        .gap(4.0)
        .align(Align::Center);
    row = match board {
        Some(b) => row.child(picker(state, theme, b)),
        None => row.child(Node::text(
            "Tasks",
            Font::UiSemibold,
            theme.font_size,
            theme.text,
        )),
    };
    row = row.child(Node::spacer());
    if board.is_none() && state.daemon_minor >= BOARDS_SINCE_MINOR {
        let new_board = if state.daemon_supports_jira() {
            Msg::OpenMenu(MenuTarget::NewBoard)
        } else {
            Msg::OpenPrompt(Prompt::NewBoard)
        };
        row = row.child(icon_button(theme, "+", "New board", new_board));
    }
    if let Some(b) = board {
        row = row.child(
            icon_button(
                theme,
                "⋯",
                "Board actions",
                Msg::OpenMenu(MenuTarget::Board(b.id.clone())),
            )
            .key(key(&["board-actions"])),
        );
    }
    row.child(icon_button(
        theme,
        "×",
        "Close tasks (Esc)",
        Msg::ToggleBoard,
    ))
}

/// "Name ▾": opens the menu of boards (and new ones).
fn picker(state: &AppState, theme: &Theme, board: &v1::Board) -> Node<Msg> {
    let many = state.workspace.boards().len() > 1;
    Node::row()
        .padding(Insets::symmetric(6.0, 10.0))
        .gap(6.0)
        .align(Align::Center)
        .radius(theme.radius)
        .background(theme.selected)
        .hover_background(theme.hover)
        .key(key(&["board-picker"]))
        .on_click(Msg::OpenMenu(MenuTarget::Boards))
        .role(Role::Button, "Choose a board")
        .children(jira::tab_badge(theme, board))
        .child(Node::text(
            shorten(&board.name, TAB_CHARS),
            Font::UiSemibold,
            theme.font_size,
            theme.text,
        ))
        .child(Node::text(
            "▾",
            Font::Ui,
            theme.font_size_small,
            if many { theme.text } else { theme.text_muted },
        ))
}

/// A tab per column, and "+" for a new one on a board of Urushi's own with room.
pub(super) fn column_tabs(state: &AppState, theme: &Theme, board: &v1::Board) -> Node<Msg> {
    let shown = state.board.shown_column(board).map(|c| c.id.as_str());
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(ROW_HEIGHT))
        .padding(Insets::symmetric(4.0, 12.0))
        .gap(4.0)
        .align(Align::Stretch)
        .children(
            board
                .columns
                .iter()
                .map(|c| column_tab(state, theme, c, shown == Some(c.id.as_str()))),
        );
    if state.daemon_supports_board_extras()
        && board.jira.is_none()
        && board.columns.len() < MAX_COLUMNS
    {
        row = row.child(Node::column().justify(Align::Center).child(icon_button(
            theme,
            "+",
            "New column",
            Msg::OpenPrompt(Prompt::NewColumn(board.id.clone())),
        )));
    }
    row
}

/// One column's tab: the count of its tasks the filter shows, its name (cut short to
/// share the width), and a line under it when it is the one shown.
fn column_tab(state: &AppState, theme: &Theme, column: &v1::BoardColumn, shown: bool) -> Node<Msg> {
    let filter = state.filter_context();
    let passing = column
        .tasks
        .iter()
        .filter(|t| state.board.filter.shows(t, filter))
        .count();
    let count = if passing == column.tasks.len() {
        passing.to_string()
    } else {
        format!("{passing}/{}", column.tasks.len())
    };
    let underline = if shown {
        super::super::labels::color(theme, column.color).unwrap_or(theme.accent)
    } else {
        Color::TRANSPARENT
    };
    let mut tab = Node::column()
        .width(Length::Fill(1.0))
        .radius(theme.radius)
        .background(if shown {
            theme.selected
        } else {
            Color::TRANSPARENT
        })
        .hover_background(if shown { theme.selected } else { theme.hover })
        .key(key(&["board-column-tab", &column.id]))
        .on_click(Msg::ShowColumn(column.id.clone()))
        .role(Role::Item, format!("Column {} tab", column.name))
        .child(
            Node::row()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .padding(Insets::symmetric(0.0, 8.0))
                .gap(6.0)
                .align(Align::Center)
                // The count first, so it stays beside the name however short that is cut.
                .child(
                    Node::row()
                        .padding(Insets::symmetric(1.0, 6.0))
                        .radius(theme.radius)
                        .background(theme.hover)
                        .child(Node::text(
                            count,
                            Font::UiMedium,
                            theme.font_size_small - 1.0,
                            if shown { theme.text } else { theme.text_muted },
                        )),
                )
                .child(
                    Node::text(
                        &column.name,
                        if shown {
                            Font::UiSemibold
                        } else {
                            Font::UiMedium
                        },
                        theme.font_size_small,
                        if shown { theme.text } else { theme.text_muted },
                    )
                    .width(Length::Fill(1.0)),
                ),
        )
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(UNDERLINE))
                .radius(UNDERLINE / 2.0)
                .background(underline),
        );
    if state.board.dragging.is_some() {
        tab = tab
            .on_drop(Msg::DropTask {
                column: column.id.clone(),
                before: None,
            })
            .hover_background(theme.accent.with_alpha(0.15));
        if state
            .board
            .over
            .as_ref()
            .is_some_and(|(c, before)| *c == column.id && before.is_none())
        {
            tab = tab.border(1.0, theme.accent.with_alpha(0.7));
        }
    }
    tab
}

/// The column shown, across the whole width.
pub(super) fn body(
    state: &AppState,
    theme: &Theme,
    board: &v1::Board,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Node<Msg> {
    let mut node = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets {
            top: 4.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        });
    if let Some(c) = state.board.shown_column(board) {
        let offset = scroll
            .get(&column_scroll_key(&c.id))
            .copied()
            .unwrap_or(0.0);
        node = node.child(super::column(state, theme, board, c, offset));
    }
    node
}
