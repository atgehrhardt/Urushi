//! The task-board panel: slides out from under the sidebar over the terminals, which
//! show faintly through it. Board tabs across the top, then the board's columns of task
//! cards, all the same size. Cards open the task editor on click and move between
//! columns by drag and drop: while a card is dragged, its place stays as a faint outline, a line shows where
//! it would land, and a raised copy follows the pointer.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Color, Font, Insets, Key, Length, Node, Role, Theme, key};

use crate::board::BOARDS_SINCE_MINOR;
use crate::msg::Msg;
use crate::organize::Prompt;
use crate::state::AppState;

/// How opaque the panel is over the terminals.
const PANEL_ALPHA: f32 = 0.86;
/// How opaque a column is (over the panel).
const COLUMN_ALPHA: f32 = 0.7;
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

pub(in crate::view) use card::drag_ghost;
use column::column;

/// Longest board name shown whole on its tab.
const TAB_CHARS: usize = 24;

/// `text`, cut to `max` characters with an ellipsis. For labels whose width comes from
/// their text (tabs, chips), which the layout doesn't shrink.
pub(super) fn shorten(text: &str, max: usize) -> String {
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
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .background(theme.background.with_alpha(PANEL_ALPHA))
        .border(1.0, theme.border)
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
    // Phones need the room for the board tabs and actions.
    if !state.compact {
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
    if state.daemon_minor >= BOARDS_SINCE_MINOR {
        row = row.child(icon_button(
            theme,
            "+",
            "New board",
            Msg::OpenPrompt(Prompt::NewBoard),
        ));
    }
    row = row.child(Node::spacer());
    if let Some(id) = shown {
        row = row
            .child(button(
                theme,
                "Rename",
                ButtonKind::Ghost,
                Msg::OpenPrompt(Prompt::RenameBoard(id.clone())),
            ))
            .child(button(
                theme,
                "Delete",
                ButtonKind::Ghost,
                Msg::OpenDeleteBoard(id),
            ));
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
