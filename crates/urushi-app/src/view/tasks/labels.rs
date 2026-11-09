//! The task editor's color and tags: a row of swatches, the task's tags as removable
//! chips with a field that adds one, and the board's other tags as suggestions. They
//! are drafts in [`TaskLabels`] until Save.

use urushi_ui::{Font, Length, Node, Role, Theme, key};

use super::super::dialogs::{field, label};
use super::super::labels::{swatches, tag_chip, tag_chip_width, wrap};
use crate::board::{TaskLabels, board_tags};
use crate::msg::{BoardMsg, Msg};
use crate::state::{AppState, Field};

/// Most suggestions shown.
const SUGGESTIONS: usize = 12;

/// "Color", the swatches, "Tags", the chips, the field and the suggestions.
pub(super) fn section(
    state: &AppState,
    theme: &Theme,
    (board, width): (&str, f32),
    labels: &TaskLabels,
) -> Vec<Node<Msg>> {
    let size = theme.font_size_small;
    let mut out = vec![
        label(theme, "Color"),
        swatches(theme, labels.color, |c| Msg::Board(BoardMsg::PickColor(c))),
        label(theme, "Tags"),
    ];
    if !labels.tags.is_empty() {
        let chips = labels
            .tags
            .iter()
            .map(|t| {
                let remove = Msg::Board(BoardMsg::RemoveTag(t.clone()));
                (
                    tag_chip_width(t, size, true),
                    tag_chip(theme, t, size, Some(remove)),
                )
            })
            .collect();
        out.push(wrap(chips, 5.0, width));
    }
    out.push(field(
        state,
        theme,
        &labels.input,
        Field::TaskTag,
        "Add a tag…",
    ));
    let suggestions: Vec<String> = state
        .workspace
        .board(board)
        .map(board_tags)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !labels.tags.iter().any(|x| x.eq_ignore_ascii_case(t)))
        .take(SUGGESTIONS)
        .collect();
    if !suggestions.is_empty() {
        let chips = suggestions
            .iter()
            .map(|t| {
                let chip = tag_chip(theme, &format!("+ {t}"), size, None)
                    .key(key(&["tag-suggestion", t]))
                    .on_click(Msg::Board(BoardMsg::AddTag(t.clone())))
                    .role(Role::Button, format!("Add tag {t}"));
                (tag_chip_width(t, size, false) + size, chip)
            })
            .collect();
        out.push(
            Node::column()
                .width(Length::Fill(1.0))
                .gap(4.0)
                .child(Node::text(
                    "On this board",
                    Font::Ui,
                    size,
                    theme.text_muted,
                ))
                .child(wrap(chips, 5.0, width)),
        );
    }
    out
}
