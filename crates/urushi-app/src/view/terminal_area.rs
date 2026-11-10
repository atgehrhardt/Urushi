//! Terminal panes on screen: the painter shared by the main area and the quick
//! terminal's panel, and the touch-screen controls that go with it.

use std::sync::Arc;

use urushi_client::ScreenState;
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{CursorIcon, Font, Insets, Key, Length, Node, Point, Role, Theme};

use crate::keys::{KeyPress, Mods, Named};
use crate::msg::Msg;
use crate::state::AppState;
use crate::surface::Surface;
use crate::terminal::{self, PaneView};

/// The panes shown on `surface`, painted by one custom painter keyed `key`. The runner
/// learns the area's rectangle from the key, and routes mouse input in it to
/// `AreaPress`/`AreaDrag`/`AreaRelease`.
pub(super) fn surface(state: &AppState, theme: &Theme, surface: Surface, key: Key) -> Node<Msg> {
    let group = state.surface_group(surface);
    let focused = group.as_ref().and_then(|(id, _)| state.focused_in(id));
    let tab = group.map(|(_, g)| g);
    let screens: Vec<(String, Option<Arc<ScreenState>>)> = tab
        .as_ref()
        .map(|t| {
            t.layout()
                .panes()
                .iter()
                .map(|p| (p.to_string(), state.screens.get(&p.to_string()).cloned()))
                .collect()
        })
        .unwrap_or_default();
    let selection = state.selection.clone();
    let palette = theme.terminal.clone();
    let (cell, font_size) = (state.cell, theme.terminal_font_size);
    let (divider, accent, muted) = (theme.glass_border, theme.accent, theme.text_muted);
    let radius = theme.radius;
    let area = Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .key(key)
        .cursor(CursorIcon::Text)
        .role(Role::Terminal, "Terminal")
        .paint(move |rect, scene, _text| {
            // One tinted sheet of glass behind every pane, with hairlines between them.
            scene.quad(urushi_ui::Quad {
                rect,
                color: palette.background,
                radius,
                ..urushi_ui::Quad::default()
            });
            let Some(tab) = &tab else { return };
            for d in terminal::dividers(tab.layout(), rect) {
                scene.rect(d.line, divider);
            }
            for (pane, pane_rect) in terminal::pane_rects(tab.layout(), rect) {
                let is_focused = focused.as_deref() == Some(&pane);
                let screen = screens
                    .iter()
                    .find(|(id, _)| *id == pane)
                    .and_then(|(_, s)| s.as_ref());
                if let Some(screen) = screen {
                    let sel = selection
                        .as_ref()
                        .filter(|(p, _)| *p == pane)
                        .map(|(_, s)| *s);
                    let view = PaneView {
                        screen,
                        palette: &palette,
                        cell,
                        font_size,
                        focused: is_focused,
                        selection: sel,
                    };
                    terminal::paint_pane(scene, pane_rect, &view);
                } else {
                    let at = Point::new(pane_rect.x + 12.0, pane_rect.y + 10.0);
                    scene.text(at, "Connecting to terminal…", Font::Ui, 12.0, muted);
                }
                if is_focused && screens.len() > 1 {
                    scene.quad(urushi_ui::Quad {
                        rect: urushi_ui::Rect::new(
                            pane_rect.x + 6.0,
                            pane_rect.y,
                            pane_rect.width - 12.0,
                            2.0,
                        ),
                        color: accent,
                        radius: 1.0,
                        ..urushi_ui::Quad::default()
                    });
                }
            }
        });
    // Inset from the panel's edges, so its rounded corners and hairline stay visible.
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 6.0,
        })
        .child(area)
}

/// Touch keyboards lack these keys; mobile clients show them above the keyboard.
pub(super) fn key_bar(theme: &Theme) -> Node<Msg> {
    let press = |named: Option<Named>, character: Option<&str>, ctrl: bool| {
        Msg::Key(KeyPress {
            named,
            character: character.map(Into::into),
            text: character.map(Into::into),
            mods: Mods {
                ctrl,
                ..Mods::default()
            },
        })
    };
    let keys = [
        ("esc", press(Some(Named::Escape), None, false)),
        ("tab", press(Some(Named::Tab), None, false)),
        ("^C", press(None, Some("c"), true)),
        ("^D", press(None, Some("d"), true)),
        ("←", press(Some(Named::Left), None, false)),
        ("↑", press(Some(Named::Up), None, false)),
        ("↓", press(Some(Named::Down), None, false)),
        ("→", press(Some(Named::Right), None, false)),
    ];
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(44.0))
        .padding(Insets::symmetric(4.0, 8.0))
        .gap(6.0)
        .children(
            keys.into_iter()
                .map(|(label, msg)| button(theme, label, ButtonKind::Secondary, msg)),
        )
}

/// Split right, split down and close for the focused pane, for touch screens, which have
/// no secondary click to open a pane's menu.
pub(super) fn pane_buttons(theme: &Theme) -> Node<Msg> {
    Node::row()
        .gap(4.0)
        .child(icon_button(
            theme,
            "→",
            "Split right",
            Msg::Split(urushi_client::v1::Direction::Right),
        ))
        .child(icon_button(
            theme,
            "↓",
            "Split down",
            Msg::Split(urushi_client::v1::Direction::Down),
        ))
        .child(icon_button(theme, "×", "Close pane", Msg::ClosePane))
}
