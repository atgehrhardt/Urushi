//! How attention looks (ADR 0017): a green dot when a thread's agent finished, an amber
//! one when something waits for the user. Shown on thread rows, tabs and the thread chip
//! of a task card.

use urushi_client::v1;
use urushi_ui::{Color, Node, Theme};

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

/// A dot for `kind`.
pub(super) fn indicator(theme: &Theme, kind: v1::AttentionKind, size: f32) -> Node<Msg> {
    dot(theme, Some(color(theme, kind)), size)
}
