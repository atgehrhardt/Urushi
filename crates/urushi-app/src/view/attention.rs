//! How attention looks (ADR 0017): a green dot when a thread's agent finished, an amber
//! one when something waits for the user. Shown on thread rows, tabs, task cards (a
//! badge and a colored border) and the thread chips and subtask rows that lead to them.

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Node, Theme};

use super::labels::dot;
use crate::msg::Msg;

/// Size of the dot on thread rows and tabs.
pub(super) const DOT: f32 = 8.0;

/// The color for `kind`.
pub(super) fn color(theme: &Theme, kind: v1::AttentionKind) -> Color {
    match kind {
        v1::AttentionKind::Done => theme.success,
        v1::AttentionKind::Waiting | v1::AttentionKind::Unspecified => theme.warning,
    }
}

/// A short word for `kind`, shown under a thread's title and read out by screen readers.
pub(super) fn label(kind: v1::AttentionKind) -> &'static str {
    match kind {
        v1::AttentionKind::Done => "finished",
        v1::AttentionKind::Waiting | v1::AttentionKind::Unspecified => "needs input",
    }
}

/// A pill with a dot and a word ("Finished", "Needs input"), for task cards.
pub(super) fn badge(theme: &Theme, kind: v1::AttentionKind) -> Node<Msg> {
    let c = color(theme, kind);
    let word = match kind {
        v1::AttentionKind::Done => "Finished",
        v1::AttentionKind::Waiting | v1::AttentionKind::Unspecified => "Needs input",
    };
    Node::row()
        .padding(Insets::symmetric(1.0, 6.0))
        .gap(4.0)
        .align(Align::Center)
        .radius(4.0)
        .background(c.with_alpha(0.16))
        .child(dot(theme, Some(c), 6.0))
        .child(Node::text(
            word,
            Font::UiMedium,
            theme.font_size_small - 0.5,
            c,
        ))
}

/// A dot for `kind`.
pub(super) fn indicator(theme: &Theme, kind: v1::AttentionKind, size: f32) -> Node<Msg> {
    dot(theme, Some(color(theme, kind)), size)
}
