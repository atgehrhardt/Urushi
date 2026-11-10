//! The error banner: the app's error ([`AppState::error`]) and a button to dismiss it.
//! [`crate::errors::error_place`] decides which surface draws it; each surface asks
//! [`for_place`] for it, so exactly one does.

use urushi_ui::widgets::{ButtonKind, button, wrapped_text};
use urushi_ui::{Align, Font, Insets, Length, Node, TextSystem, Theme, key};

use crate::errors::{ErrorPlace, error_place};
use crate::msg::Msg;
use crate::state::AppState;

/// Most lines a banner grows to before cutting the message short.
const MAX_LINES: usize = 6;
/// The banner's padding: (vertical, horizontal).
const PADDING: (f32, f32) = (8.0, 16.0);
/// Room the "Dismiss" button and the gap before it take beside the message.
const DISMISS_ROOM: f32 = 90.0;

/// The banner, if there is an error and `place` is the surface that draws it.
/// `width` is the banner's width when the surface knows it (dialogs): the message then
/// wraps onto more lines; otherwise it takes one line.
pub(super) fn for_place(
    state: &AppState,
    theme: &Theme,
    place: ErrorPlace,
    width: Option<f32>,
) -> Option<Node<Msg>> {
    let message = state.error.as_deref()?;
    (error_place(state) == place).then(|| banner(theme, message, width))
}

/// The danger-tinted banner with the message and "Dismiss".
fn banner(theme: &Theme, message: &str, width: Option<f32>) -> Node<Msg> {
    let text = match width {
        Some(width) => Node::column()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(text_height(theme, message, width)))
            .child(wrapped_text(message, Font::Ui, theme.font_size, theme.text)),
        None => Node::text(message, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)),
    };
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets::symmetric(PADDING.0, PADDING.1))
        .gap(12.0)
        .align(Align::Center)
        .background(theme.danger.mix(theme.background, 0.7))
        .radius(theme.radius)
        .key(key(&["error-banner"]))
        // Clicks beside the button stay in the banner (not the board or a terminal
        // underneath).
        .opaque()
        .child(text)
        .child(button(
            theme,
            "Dismiss",
            ButtonKind::Ghost,
            Msg::DismissError,
        ))
}

/// The banner's height when it shows `message` in a dialog `width` wide, so dialogs
/// that fill the window can leave room for it.
pub(super) fn height_in_dialog(theme: &Theme, message: &str, width: f32) -> f32 {
    // The text, or the button when it's taller, plus the padding.
    let button = TextSystem::line_height(theme.font_size) + 12.0;
    text_height(theme, message, width).max(button) + 2.0 * PADDING.0
}

/// The height of the message's lines in a banner `width` wide.
fn text_height(theme: &Theme, message: &str, width: f32) -> f32 {
    let room = width - 2.0 * PADDING.1 - DISMISS_ROOM;
    #[allow(clippy::cast_precision_loss, reason = "at most MAX_LINES lines")]
    let lines = estimated_lines(message, theme.font_size, room) as f32;
    lines * TextSystem::line_height(theme.font_size)
}

/// About how many lines `message` wraps onto in `width` (the view has no text
/// measurer): from an average character width, at least 1 and at most [`MAX_LINES`].
fn estimated_lines(message: &str, size: f32, width: f32) -> usize {
    // Wide enough for most UI text; wrapping early only leaves a little space.
    let per_char = size * 0.55;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small non-negative count"
    )]
    let per_line = ((width.max(per_char) / per_char).floor() as usize).max(1);
    // Lines break between words, so they hold a little less than they could.
    let lines: usize = message
        .split('\n')
        .map(|l| (l.chars().count() * 6 / 5).div_ceil(per_line).max(1))
        .sum();
    lines.clamp(1, MAX_LINES)
}

#[cfg(test)]
mod tests;
