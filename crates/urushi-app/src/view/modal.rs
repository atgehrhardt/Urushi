//! The app's dialogs: `urushi-ui`'s modal, with the app's error between the body and
//! the buttons while the dialog is on top (see [`crate::errors`]). Every dialog is
//! built through here, so an error never hides behind one.

use urushi_ui::widgets;
use urushi_ui::{Length, Node, Theme};

use super::error;
use crate::errors::ErrorPlace;
use crate::msg::Msg;
use crate::state::AppState;

/// The modal card's padding on each side (`widgets::modal_with_header`).
const CARD_PADDING: f32 = 20.0;
/// Space between the body and the error.
const GAP: f32 = 12.0;

/// [`widgets::modal`], showing the error under `body`.
pub(super) fn modal(
    state: &AppState,
    theme: &Theme,
    title: &str,
    width: f32,
    body: Node<Msg>,
    actions: Vec<Node<Msg>>,
    on_dismiss: Option<Msg>,
) -> Node<Msg> {
    let body = with_error(state, theme, width, body);
    widgets::modal(theme, title, width, body, actions, on_dismiss)
}

/// [`widgets::modal_with_header`], showing the error under `body`.
pub(super) fn modal_with_header(
    state: &AppState,
    theme: &Theme,
    header: (Node<Msg>, &str),
    width: f32,
    body: Node<Msg>,
    actions: Vec<Node<Msg>>,
    on_dismiss: Option<Msg>,
) -> Node<Msg> {
    let body = with_error(state, theme, width, body);
    widgets::modal_with_header(theme, header, width, body, actions, on_dismiss)
}

/// The height the error takes in a dialog `width` wide (0 without one), for dialogs
/// that size their body to the window.
pub(super) fn error_height(state: &AppState, theme: &Theme, width: f32) -> f32 {
    match state.error.as_deref() {
        Some(message) => error::height_in_dialog(theme, message, inner(width)) + GAP,
        None => 0.0,
    }
}

fn with_error(state: &AppState, theme: &Theme, width: f32, body: Node<Msg>) -> Node<Msg> {
    match error::for_place(state, theme, ErrorPlace::Dialog, Some(inner(width))) {
        Some(banner) => Node::column()
            .width(Length::Fill(1.0))
            .gap(GAP)
            .child(body)
            .child(banner),
        None => body,
    }
}

/// The width inside the card.
fn inner(width: f32) -> f32 {
    width - 2.0 * CARD_PADDING
}
