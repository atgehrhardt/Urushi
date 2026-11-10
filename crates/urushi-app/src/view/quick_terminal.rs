//! The quick terminal (ADR 0031): its drop-down panel over the main area, and the
//! button that drops it down or puts it away.

use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Font, Insets, Length, Node, Role, Theme, key};

use super::quick_area_key;
use crate::msg::{Msg, QuickMsg};
use crate::quick_terminal::HEIGHT_SHARE;
use crate::state::{AppState, Connection};
use crate::surface::Surface;

/// The shortcut, as shown in labels.
fn shortcut() -> &'static str {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        "⌘`"
    } else {
        "Ctrl+Shift+`"
    }
}

/// The `>_` button that toggles the panel, with a dot when a quick-terminal pane wants
/// the user.
pub(super) fn toggle_button(state: &AppState, theme: &Theme) -> Node<Msg> {
    let label = if state.quick.open {
        format!("Hide quick terminal ({})", shortcut())
    } else {
        format!("Quick terminal ({})", shortcut())
    };
    Node::row()
        .align(Align::Center)
        .gap(2.0)
        .children(
            state
                .quick_indicator()
                .map(|k| super::attention::indicator(state, theme, k, super::attention::DOT)),
        )
        .child(
            icon_button(theme, ">_", &label, Msg::Quick(QuickMsg::Toggle))
                .key(key(&["quick-terminal-button"])),
        )
}

/// The panel, dropped down by `state.quick.eased()`, over the top of the main area,
/// and a dimmed strip under it that puts it away when clicked. Place it over the main
/// area in a clipped stack.
pub(super) fn panel(state: &AppState, theme: &Theme) -> Node<Msg> {
    let eased = state.quick.eased();
    let radius = if state.compact {
        0.0
    } else {
        theme.radius_large
    };
    let mut card = urushi_ui::widgets::overlay(
        theme,
        Node::column()
            .width(Length::Fill(1.0))
            .height(Length::Fill(HEIGHT_SHARE)),
        radius,
    )
    .key(key(&["quick-panel"]))
    .role(Role::Dialog, "Quick terminal")
    // Clicks on the panel's own chrome stay in it, but nothing on it takes clicks itself:
    // presses on its terminals go to the terminal area (`AreaPress`).
    .opaque()
    .slide_y(1.0 - eased)
    .child(header(state, theme))
    .child(urushi_ui::widgets::divider(theme, false))
    .child(body(state, theme));
    if cfg!(any(target_os = "ios", target_os = "android")) {
        card = card.child(super::terminal_area::key_bar(theme));
    }
    let scrim = Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .background(theme.scrim.with_alpha(theme.scrim.a * 0.6 * eased))
        .key(key(&["quick-scrim"]))
        .on_click(Msg::Quick(QuickMsg::Hide))
        .role(Role::Button, "Hide quick terminal");
    Node::stack()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .child(scrim)
        .child(
            Node::column()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .child(card)
                // Under the panel, clicks fall through to the strip that hides it.
                .child(
                    Node::new()
                        .width(Length::Fill(1.0))
                        .height(Length::Fill(1.0 - HEIGHT_SHARE)),
                ),
        )
}

/// Title, pane buttons on touch screens, and Hide.
fn header(state: &AppState, theme: &Theme) -> Node<Msg> {
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(40.0))
        .padding(Insets::symmetric(0.0, 10.0))
        .gap(6.0)
        .align(Align::Center);
    // The panel covers the tab bar and its "Show sidebar" button, so it has its own.
    if !state.compact && state.sidebar.collapsed() {
        row = row.child(super::sidebar_toggle(state, theme));
    }
    row = row
        .child(Node::text(
            "Quick terminal",
            Font::UiSemibold,
            theme.font_size,
            theme.text,
        ))
        .child(Node::text(
            shortcut(),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
        .child(Node::spacer());
    if !crate::platform::HAS_SECONDARY_CLICK {
        row = row.child(super::terminal_area::pane_buttons(theme));
    }
    row.child(button(
        theme,
        "Hide",
        ButtonKind::Ghost,
        Msg::Quick(QuickMsg::Hide),
    ))
}

/// The quick terminal's panes, or why there are none yet.
fn body(state: &AppState, theme: &Theme) -> Node<Msg> {
    let waiting = |text: &str| {
        Node::column()
            .width(Length::Fill(1.0))
            .height(Length::Fill(1.0))
            .justify(Align::Center)
            .align(Align::Center)
            .child(Node::text(
                text,
                Font::Ui,
                theme.font_size,
                theme.text_muted,
            ))
    };
    if !matches!(state.connection, Connection::Connected { .. }) {
        return waiting("Connecting…");
    }
    if state.workspace.quick_terminal_raw().is_none() {
        return waiting("Starting a terminal…");
    }
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets {
            top: 6.0,
            right: 6.0,
            bottom: 6.0,
            left: 6.0,
        })
        .child(super::terminal_area::surface(
            state,
            theme,
            Surface::Quick,
            quick_area_key(),
        ))
}
