//! One column of the board: its header (name, count, new task) and its cards, with the
//! drop targets that place a dragged card.

use urushi_client::v1;
use urushi_ui::widgets::icon_button;
use urushi_ui::{Font, Insets, Length, Node, Theme, key};

use super::card::card;
use super::{CARD_GAP, COLUMN_ALPHA, column_scroll_key};
use crate::msg::Msg;
use crate::state::AppState;

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
    let head = Node::row()
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
        )
        .child(Node::text(
            column.tasks.len().to_string(),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
        .child(icon_button(
            theme,
            "+",
            &format!("New task in {}", column.name),
            Msg::NewTask {
                board: board.id.clone(),
                column: column.id.clone(),
            },
        ));
    // Each card sits in a slot that includes the space under it, and the drop line in
    // one that drops in the same place, so the pointer is always over some target and
    // the line doesn't flicker as it moves the cards.
    let mut cards: Vec<Node<Msg>> = Vec::new();
    for t in &column.tasks {
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
        .padding(Insets {
            top: 4.0,
            right: 0.0,
            bottom: 8.0,
            left: 0.0,
        })
        .radius(theme.radius + 2.0)
        .background(theme.sidebar.with_alpha(COLUMN_ALPHA))
        .border(
            1.0,
            if over.is_some() {
                theme.accent.with_alpha(0.7)
            } else {
                theme.border
            },
        )
        .key(key(&["board-column-box", &column.id]))
        .child(head)
        .child(cards);
    if dragging {
        node = node
            .on_drop(drop_here)
            .hover_background(theme.sidebar.mix(theme.hover, 0.6).with_alpha(0.85));
    }
    node
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

/// Where a dragged card would land.
fn drop_line(theme: &Theme) -> Node<Msg> {
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(3.0))
        .radius(1.5)
        .background(theme.accent)
}
