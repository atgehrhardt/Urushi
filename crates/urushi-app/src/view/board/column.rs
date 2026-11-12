//! One column of the board: its header (name, count, its menu, new task), its color,
//! and its cards (those passing the filter), with the drop targets that place a dragged
//! card. Only the cards near what the column shows are built; spacers stand in for the
//! rest, so a column of hundreds of cards costs what a screenful does (UR-66).

use urushi_client::v1;
use urushi_ui::widgets::icon_button;
use urushi_ui::{Font, Insets, Length, Node, Theme, key};

use super::super::labels;
use super::card::{card, card_height};
use super::{CARD_GAP, column_scroll_key};
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
    let filter = &state.board.filter;
    let passing = filter.active().then(|| {
        state
            .board
            .filtered
            .shown(board, filter, state.filter_context())
    });
    let shown: Vec<&v1::Task> = column
        .tasks
        .iter()
        .filter(|t| passing.as_ref().is_none_or(|p| p.contains(&t.id)))
        .collect();
    let count = if shown.len() == column.tasks.len() {
        column.tasks.len().to_string()
    } else {
        format!("{}/{}", shown.len(), column.tasks.len())
    };
    let tint = labels::color(theme, column.color);
    let head = header(state, theme, (board, column), count);
    let cards = cards(state, theme, (column, shown), offset);
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

/// The column's cards that can be in sight at `offset`, each in its slot, with the drop
/// line where a dragged card would land, and spacers for the rest.
fn cards(
    state: &AppState,
    theme: &Theme,
    (column, shown): (&v1::BoardColumn, Vec<&v1::Task>),
    offset: f32,
) -> Vec<Node<Msg>> {
    let dragging = state.board.dragging.is_some();
    let over = state
        .board
        .over
        .as_ref()
        .filter(|(c, _)| *c == column.id)
        .map(|(_, before)| before.as_deref());
    let drop_before = |before: Option<&v1::Task>| Msg::DropTask {
        column: column.id.clone(),
        before: before.map(|t| t.id.clone()),
    };
    // Each card sits in a slot that includes the space under it, and the drop line in
    // one that drops in the same place, so the pointer is always over some target and
    // the line doesn't flicker as it moves the cards.
    let mut rows: Vec<Row<'_>> = Vec::with_capacity(shown.len() + 1);
    for t in shown {
        if over == Some(Some(t.id.as_str())) {
            rows.push(Row::Line(Some(t)));
        }
        rows.push(Row::Card(t));
    }
    if over == Some(None) {
        rows.push(Row::Line(None));
    }
    let slot_height = card_height(theme) + CARD_GAP;
    let line_height = DROP_LINE + CARD_GAP;
    let heights: Vec<f32> = rows
        .iter()
        .map(|r| match r {
            Row::Card(_) => slot_height,
            Row::Line(_) => line_height,
        })
        .collect();
    let (range, above, below) = window(&heights, offset, state.window_height);
    let mut cards: Vec<Node<Msg>> = Vec::with_capacity(range.len() + 2);
    if above > 0.0 {
        cards.push(Node::new().height(Length::Fixed(above)));
    }
    for row in &rows[range] {
        cards.push(match row {
            Row::Card(t) => slot(dragging, drop_before(Some(t))).child(card(state, theme, t)),
            Row::Line(before) => slot(dragging, drop_before(*before)).child(drop_line(theme)),
        });
    }
    if below > 0.0 {
        cards.push(Node::new().height(Length::Fixed(below)));
    }
    cards
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
    head = head.child(Node::text(
        count,
        Font::Ui,
        theme.font_size_small,
        theme.text_muted,
    ));
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

/// What a column lists: a card, or the line showing where a dragged one would land
/// (before that card, or at the end).
enum Row<'a> {
    Card(&'a v1::Task),
    Line(Option<&'a v1::Task>),
}

/// Height of the drop line.
const DROP_LINE: f32 = 3.0;

/// Which of the rows of `heights` to build for a list scrolled to `offset` in a window
/// `view` tall (the list's own height is at most that), with the space taken by the
/// rows before and after them. Covers wherever the layout may clamp `offset` to, plus
/// a row either side. A `view` of zero (not known yet) builds everything.
fn window(heights: &[f32], offset: f32, view: f32) -> (std::ops::Range<usize>, f32, f32) {
    let total: f32 = heights.iter().sum();
    if view <= 0.0 {
        return (0..heights.len(), 0.0, 0.0);
    }
    // The layout clamps the offset to `total - list height`, which is at least
    // `total - view`.
    let low = offset.min((total - view).max(0.0)).max(0.0);
    let high = offset.max(low) + view;
    // The first row reaching past `low` and the last starting before `high`, and one
    // more either side.
    let (mut first, mut last, mut y) = (None, 0, 0.0);
    for (i, h) in heights.iter().enumerate() {
        if first.is_none() && y + h > low {
            first = Some(i);
        }
        if y < high {
            last = i;
        }
        y += h;
    }
    let start = first.map_or(heights.len(), |f| f.saturating_sub(1));
    let end = if heights.is_empty() {
        0
    } else {
        (last + 2).min(heights.len()).max(start)
    };
    let above: f32 = heights[..start].iter().sum();
    let shown: f32 = heights[start..end].iter().sum();
    (start..end, above, (total - above - shown).max(0.0))
}

/// The accent line that shows where a dragged card or project would land.
pub(in crate::view) fn drop_line(theme: &Theme) -> Node<Msg> {
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(DROP_LINE))
        .radius(1.5)
        .background(theme.accent)
}

#[cfg(test)]
mod tests {
    use super::window;

    #[test]
    #[allow(clippy::float_cmp, reason = "sums of whole numbers are exact")]
    fn only_the_rows_in_sight_are_built_and_spacers_keep_the_rest() {
        let rows = vec![10.0; 100];
        // At the top of a 35 tall window: rows 0..=4 (one past what shows).
        let (range, above, below) = window(&rows, 0.0, 35.0);
        assert_eq!((range.clone(), above), (0..5, 0.0));
        assert_eq!(below, 950.0);
        // Scrolled to 500: one row before, one after.
        let (range, above, below) = window(&rows, 500.0, 35.0);
        assert_eq!(range, 49..55);
        assert_eq!(above + rows[range].iter().sum::<f32>() + below, 1000.0);
        assert_eq!(above, 490.0);
        // An offset past the end (the list got shorter) still covers where the
        // layout clamps it to.
        let (range, _, below) = window(&rows[..10], 5000.0, 35.0);
        assert!(range.contains(&6) && range.contains(&9), "{range:?}");
        assert_eq!(below, 0.0);
        // Unknown window height: everything.
        assert_eq!(window(&rows, 0.0, 0.0), (0..100, 0.0, 0.0));
        assert_eq!(window(&[], 10.0, 35.0), (0..0, 0.0, 0.0));
    }
}
