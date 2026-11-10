//! One column of the board: its header (name, count, its menu, new task), its color,
//! and its cards (those passing the tag filter), with the drop targets that place a
//! dragged card.

use urushi_client::v1;
use urushi_ui::widgets::icon_button;
use urushi_ui::{Font, Insets, Length, Node, Theme, key};

use super::super::labels;
use super::card::card;
use super::{CARD_GAP, column_scroll_key};
use crate::board::shows;
use crate::msg::Msg;
use crate::organize::MenuTarget;
use crate::state::AppState;

/// How much a column's color tints its background.
const TINT: f32 = 0.08;
/// Height of a colored column's bar.
const COLOR_BAR: f32 = 3.0;

pub(super) fn column(
    state: &AppState,
    theme: &Theme,
    board: &v1::Board,
    column: &v1::BoardColumn,
    offset: f32,
) -> Node<Msg> {
    let dragging = state.board.dragging.is_some();
    let over = state
        .board
        .over
        .as_ref()
        .filter(|(c, _)| *c == column.id)
        .map(|(_, before)| before.as_deref());
    let drop_here = Msg::DropTask {
        column: column.id.clone(),
        before: None,
    };
    let shown: Vec<&v1::Task> = column
        .tasks
        .iter()
        .filter(|t| shows(state.board.tag_filter.as_deref(), t))
        .collect();
    let count = if shown.len() == column.tasks.len() {
        column.tasks.len().to_string()
    } else {
        format!("{}/{}", shown.len(), column.tasks.len())
    };
    let tint = labels::color(theme, column.color);
    let head = header(state, theme, (board, column), count);
    // Each card sits in a slot that includes the space under it, and the drop line in
    // one that drops in the same place, so the pointer is always over some target and
    // the line doesn't flicker as it moves the cards.
    let mut cards: Vec<Node<Msg>> = Vec::new();
    for t in shown {
        let onto = Msg::DropTask {
            column: column.id.clone(),
            before: Some(t.id.clone()),
        };
        if over == Some(Some(t.id.as_str())) {
            cards.push(slot(dragging, onto.clone()).child(drop_line(theme)));
        }
        cards.push(slot(dragging, onto).child(card(state, theme, t)));
    }
    if over == Some(None) {
        cards.push(slot(dragging, drop_here.clone()).child(drop_line(theme)));
    }
    let cards = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets::symmetric(2.0, 8.0))
        .scroll(column_scroll_key(&column.id), offset)
        .children(cards);
    let mut node = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        // A colored column's bar takes most of the top padding, so cards line up
        // across columns.
        .padding(Insets {
            top: if tint.is_some() { 4.0 - COLOR_BAR } else { 4.0 },
            right: 0.0,
            bottom: 8.0,
            left: 0.0,
        })
        .radius(theme.radius + 2.0)
        .background(tint.map_or(theme.surface, |c| c.with_alpha(TINT)))
        .border(
            1.0,
            if over.is_some() {
                theme.accent.with_alpha(0.7)
            } else {
                theme.border
            },
        )
        .key(key(&["board-column-box", &column.id]));
    // A colored column has a bar of its color across the top.
    if let Some(c) = tint {
        node = node.child(
            Node::row()
                .width(Length::Fill(1.0))
                .padding(Insets::symmetric(0.0, 10.0))
                .child(
                    Node::new()
                        .width(Length::Fill(1.0))
                        .height(Length::Fixed(COLOR_BAR))
                        .radius(COLOR_BAR / 2.0)
                        .background(c),
                ),
        );
    }
    node = node.child(head).child(cards);
    if dragging {
        node = node.on_drop(drop_here).hover_background(theme.selected);
    }
    node
}

/// The name, the count of tasks shown, the column's menu (⋯, or a right-click on the
/// header) and "+" for a new task.
fn header(
    state: &AppState,
    theme: &Theme,
    (board, column): (&v1::Board, &v1::BoardColumn),
    count: String,
) -> Node<Msg> {
    let extras = state.daemon_supports_board_extras();
    let mut head = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(34.0))
        .padding(Insets {
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
            left: 12.0,
        })
        .gap(6.0)
        // Fill, so a long name is cut short before the count and the buttons.
        .child(
            Node::text(
                &column.name,
                Font::UiSemibold,
                theme.font_size_small,
                theme.text,
            )
            .width(Length::Fill(1.0)),
        );
    // Phones have little room for the name; the count goes first.
    if !state.compact {
        head = head.child(Node::text(
            count,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ));
    }
    if extras {
        let menu = Msg::OpenMenu(MenuTarget::Column(column.id.clone()));
        head = head.on_secondary_click(menu.clone()).child(icon_button(
            theme,
            "⋯",
            &format!("Column {} actions", column.name),
            menu,
        ));
    }
    head.child(icon_button(
        theme,
        "+",
        &format!("New task in {}", column.name),
        Msg::NewTask {
            board: board.id.clone(),
            column: column.id.clone(),
        },
    ))
}

/// A card's place in a column: the card and the gap under it. While a card is dragged,
/// it drops there.
fn slot(dragging: bool, drop: Msg) -> Node<Msg> {
    let node = Node::column().width(Length::Fill(1.0)).padding(Insets {
        top: 0.0,
        right: 0.0,
        bottom: CARD_GAP,
        left: 0.0,
    });
    if dragging { node.on_drop(drop) } else { node }
}

/// The accent line that shows where a dragged card or project would land.
pub(in crate::view) fn drop_line(theme: &Theme) -> Node<Msg> {
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(3.0))
        .radius(1.5)
        .background(theme.accent)
}
