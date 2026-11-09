//! The main area: tab bar, terminal panes, the status bar, and the error banner.

use std::sync::Arc;

use urushi_client::ScreenState;
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Color, CursorIcon, Font, Insets, Length, Node, Point, Role, Theme, key};

use super::area_key;
use crate::keys::{KeyPress, Mods, Named};
use crate::msg::Msg;
use crate::state::{AppState, Connection};
use crate::terminal::{self, PaneView};

pub(super) fn view(state: &AppState, theme: &Theme) -> Node<Msg> {
    let mut column = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0));
    if state.current_tab().is_some() {
        column = column
            .child(tab_bar(state, theme))
            .child(terminal_area(state, theme))
            .child(super::status_bar::view(state, theme));
        if cfg!(any(target_os = "ios", target_os = "android")) {
            column = column.child(key_bar(theme));
        }
    } else if let Some(thread) = state.empty_thread() {
        // A new thread, or one whose last tab was closed: keep the tab bar (and its "+")
        // and offer what to run.
        column = column
            .child(tab_bar(state, theme))
            .child(super::launcher::inline(state, theme, &thread.title));
    } else {
        column = column.child(empty_state(state, theme));
    }
    if let Some(error) = &state.error {
        column = column.child(
            Node::row()
                .width(Length::Fill(1.0))
                .padding(Insets::symmetric(8.0, 16.0))
                .gap(12.0)
                .background(theme.danger.mix(theme.background, 0.7))
                .child(
                    Node::text(error, Font::Ui, theme.font_size, theme.text)
                        .width(Length::Fill(1.0)),
                )
                .child(button(
                    theme,
                    "Dismiss",
                    ButtonKind::Ghost,
                    Msg::DismissError,
                )),
        );
    }
    column
}

fn empty_state(state: &AppState, theme: &Theme) -> Node<Msg> {
    let (title, detail) = match &state.connection {
        Connection::Disconnected(reason) => ("Not connected to the Urushi daemon", reason.clone()),
        Connection::Connecting => ("Connecting…", String::new()),
        Connection::Connected { .. } if state.workspace.repos().is_empty() => (
            "Add a repository to get started",
            "Each thread gets its own git worktree and branch.".to_owned(),
        ),
        Connection::Connected { .. } => (
            "Create a thread",
            "Threads are isolated worktrees with their own terminals.".to_owned(),
        ),
    };
    let mut actions = Node::row().gap(8.0);
    if matches!(state.connection, Connection::Connected { .. }) {
        actions = actions.child(button(
            theme,
            "Add repo",
            ButtonKind::Secondary,
            Msg::OpenAddProject,
        ));
        if !state.workspace.repos().is_empty() {
            actions = actions.child(button(
                theme,
                "New thread",
                ButtonKind::Primary,
                Msg::OpenNewThread {
                    repo: None,
                    project: None,
                },
            ));
        }
    } else {
        actions = actions.child(button(
            theme,
            "Connect to a host…",
            ButtonKind::Secondary,
            Msg::OpenConnect,
        ));
    }
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .justify(Align::Center)
        .align(Align::Center)
        .gap(10.0)
        .child(Node::text(
            title,
            Font::UiSemibold,
            theme.font_size + 4.0,
            theme.text,
        ))
        .child(Node::text(
            detail,
            Font::Ui,
            theme.font_size,
            theme.text_muted,
        ))
        .child(Node::new().height(Length::Fixed(6.0)))
        .child(actions)
}

fn tab_bar(state: &AppState, theme: &Theme) -> Node<Msg> {
    let current = state.current_tab().map(|t| t.id.clone());
    let thread = state.selected_thread.clone().unwrap_or_default();
    let tabs = state.workspace.tabs_of(&thread).into_iter().map(|tab| {
        let selected = current.as_deref() == Some(&tab.id);
        Node::row()
            .height(Length::Fill(1.0))
            .padding(Insets {
                top: 0.0,
                right: 4.0,
                bottom: 0.0,
                left: 12.0,
            })
            .gap(6.0)
            .background(if selected {
                theme.background
            } else {
                Color::TRANSPARENT
            })
            .hover_background(if selected {
                theme.background
            } else {
                theme.hover
            })
            .key(key(&["tab", &tab.id]))
            .on_click(Msg::SelectTab(tab.id.clone()))
            .role(Role::Item, format!("Tab {}", tab.title))
            .children(
                state
                    .tab_attention(&tab.id)
                    .map(|k| super::attention::indicator(theme, k, super::attention::DOT)),
            )
            .child(Node::text(
                &tab.title,
                Font::UiMedium,
                theme.font_size,
                if selected {
                    theme.text
                } else {
                    theme.text_muted
                },
            ))
            .child(icon_button(
                theme,
                "×",
                &format!("Close tab {}", tab.title),
                Msg::CloseTab(tab.id.clone()),
            ))
    });
    // No thread title here: the sidebar already shows which thread is selected.
    let mut bar = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(36.0))
        .background(theme.sidebar)
        .align(Align::Stretch);
    if state.compact {
        bar = bar.child(
            Node::row()
                .padding(Insets::symmetric(0.0, 6.0))
                .child(icon_button(theme, "‹", "Back to threads", Msg::ShowSidebar)),
        );
    }
    let bar = bar.children(tabs).child(
        Node::row()
            .padding(Insets::symmetric(0.0, 6.0))
            .child(icon_button(theme, "+", "New tab…", Msg::NewTab)),
    );
    // Splitting and closing panes is in each pane's context menu (a right or two-finger
    // click). Touch screens have no such click, so they keep buttons for the focused pane.
    if crate::platform::HAS_SECONDARY_CLICK {
        return bar;
    }
    bar.child(Node::spacer()).child(
        Node::row()
            .padding(Insets::symmetric(0.0, 10.0))
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
            .child(icon_button(theme, "×", "Close pane", Msg::ClosePane)),
    )
}

/// The panes of the current tab, painted by one custom painter. The runner routes mouse
/// input in this area to `AreaPress`/`AreaDrag`/`AreaRelease`.
fn terminal_area(state: &AppState, theme: &Theme) -> Node<Msg> {
    let tab = state.current_tab().and_then(|t| state.workspace.tab(&t.id));
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
    let focused = state.focused_pane();
    let selection = state.selection.clone();
    let palette = theme.terminal.clone();
    let (cell, font_size) = (state.cell, theme.terminal_font_size);
    let (divider, accent, muted) = (theme.border, theme.accent, theme.text_muted);
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .key(area_key())
        .cursor(CursorIcon::Text)
        .role(Role::Terminal, "Terminal")
        .paint(move |rect, scene, _text| {
            let Some(tab) = &tab else { return };
            scene.rect(rect, divider);
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
                    scene.rect(pane_rect, palette.background);
                    let at = Point::new(pane_rect.x + 12.0, pane_rect.y + 10.0);
                    scene.text(at, "Connecting to terminal…", Font::Ui, 12.0, muted);
                }
                if is_focused && screens.len() > 1 {
                    scene.rect(
                        urushi_ui::Rect::new(pane_rect.x, pane_rect.y, pane_rect.width, 2.0),
                        accent,
                    );
                }
            }
        })
}

/// Touch keyboards lack these keys; mobile clients show them above the keyboard.
fn key_bar(theme: &Theme) -> Node<Msg> {
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
        .background(theme.sidebar)
        .children(
            keys.into_iter()
                .map(|(label, msg)| button(theme, label, ButtonKind::Secondary, msg)),
        )
}
