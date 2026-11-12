//! The main area: tab bar, terminal panes, the status bar, and the error when nothing
//! covers the main area (`errors.rs`).

use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::area_key;
use crate::msg::Msg;
use crate::state::{AppState, Connection};
use crate::surface::Surface;

pub(super) fn view(state: &AppState, theme: &Theme) -> Node<Msg> {
    let mut column = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0));
    if state.current_tab().is_some() {
        column = column
            .child(tab_bar(state, theme))
            .child(super::terminal_area::surface(
                state,
                theme,
                Surface::Main,
                area_key(),
            ))
            .child(super::status_bar::view(state, theme));
        if cfg!(any(target_os = "ios", target_os = "android")) {
            column = column.child(super::terminal_area::key_bar(theme));
        }
    } else if let Some(pending) = state.shown_pending() {
        column = column.child(creating(theme, &pending.title));
    } else if let Some(thread) = state.empty_thread() {
        // A new thread, or one whose last tab was closed: keep the tab bar (and its "+")
        // and offer what to run, unless the first tab is on its way already.
        column = column.child(tab_bar(state, theme));
        column = match state.starting.tabs_of(&thread.id).next() {
            Some(tab) => column.child(starting_tab(theme, &tab.title)),
            None => column.child(super::launcher::inline(state, theme, &thread.title)),
        };
    } else {
        // Without a tab bar, the button that shows a collapsed sidebar goes on top.
        if !state.compact && state.sidebar.collapsed() {
            column = column.child(
                Node::row()
                    .width(Length::Fill(1.0))
                    .height(Length::Fixed(40.0))
                    .padding(Insets::symmetric(0.0, 8.0))
                    .align(Align::Center)
                    .child(super::sidebar_toggle(state, theme)),
            );
        }
        column = column.child(empty_state(state, theme));
    }
    column.children(super::error::for_place(
        state,
        theme,
        crate::errors::ErrorPlace::Main,
        None,
    ))
}

/// A thread the daemon is still creating (ADR 0055): the base is updated from its
/// remote and the worktree made, which can take a few seconds.
fn creating(theme: &Theme, title: &str) -> Node<Msg> {
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .justify(Align::Center)
        .align(Align::Center)
        .gap(10.0)
        .key(key(&["creating-thread"]))
        .role(Role::Label, format!("Creating thread {title}"))
        .child(Node::text(
            format!("Creating \u{201c}{title}\u{201d}\u{2026}"),
            Font::UiSemibold,
            theme.font_size + 4.0,
            theme.text,
        ))
        .child(Node::text(
            "Updating its base from the remote and making its worktree. It opens here when ready.",
            Font::Ui,
            theme.font_size,
            theme.text_muted,
        ))
}

/// A tab in the tab bar while the daemon starts it.
fn starting_chip(theme: &Theme, tab: &crate::starting::StartingTab) -> Node<Msg> {
    Node::row()
        .height(Length::Fill(1.0))
        .padding(Insets::symmetric(0.0, 12.0))
        .align(Align::Center)
        .key(key(&["starting-tab", &tab.key.to_string()]))
        .role(Role::Label, format!("Starting {}", tab.title))
        .child(Node::text(
            format!("{}\u{2026}", tab.title),
            Font::UiMedium,
            theme.font_size,
            theme.text_muted,
        ))
}

/// The first tab of a thread, while the daemon starts it.
fn starting_tab(theme: &Theme, title: &str) -> Node<Msg> {
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .justify(Align::Center)
        .align(Align::Center)
        .key(key(&["starting-first-tab"]))
        .child(Node::text(
            format!("Starting {title}\u{2026}"),
            Font::UiMedium,
            theme.font_size + 2.0,
            theme.text_muted,
        ))
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
                right: 3.0,
                bottom: 0.0,
                left: 12.0,
            })
            .gap(6.0)
            .radius(theme.radius + 1.0)
            .background(if selected {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(if selected {
                theme.selected
            } else {
                theme.hover
            })
            .key(key(&["tab", &tab.id]))
            .on_click(Msg::SelectTab(tab.id.clone()))
            .role(Role::Item, format!("Tab {}", tab.title))
            .children(
                state
                    .tab_indicator(&tab.id)
                    .map(|k| super::attention::indicator(state, theme, k, super::attention::DOT)),
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
    // No thread title here: the sidebar already shows which thread is selected. The
    // bar sits on the main panel's glass, with the tabs as pills.
    let mut bar = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(40.0))
        .padding(Insets::symmetric(6.0, 6.0))
        .gap(2.0)
        .align(Align::Stretch);
    if state.compact {
        bar = bar.child(Node::row().align(Align::Center).child(icon_button(
            theme,
            "‹",
            "Back to threads",
            Msg::ShowSidebar,
        )));
    } else if state.sidebar.collapsed() {
        bar = bar.child(
            Node::row()
                .align(Align::Center)
                .padding(Insets::symmetric(0.0, 2.0))
                .child(super::sidebar_toggle(state, theme)),
        );
    }
    // Tabs the daemon is still starting (`starting.rs`).
    let starting = state
        .starting
        .tabs_of(&thread)
        .map(|t| starting_chip(theme, t));
    let mut bar = bar.children(tabs).children(starting).child(
        Node::row()
            .align(Align::Center)
            .padding(Insets::symmetric(0.0, 4.0))
            .child(icon_button(theme, "+", "New tab…", Msg::NewTab)),
    );
    bar = bar.child(Node::spacer());
    // Splitting and closing panes is in each pane's context menu (a right or two-finger
    // click). Touch screens have no such click, so they keep buttons for the focused pane.
    if !crate::platform::HAS_SECONDARY_CLICK {
        bar = bar.child(
            Node::row()
                .align(Align::Center)
                .padding(Insets::symmetric(0.0, 6.0))
                .child(super::terminal_area::pane_buttons(theme)),
        );
    }
    bar.children(state.daemon_supports_quick_terminal().then(|| {
        Node::row()
            .align(Align::Center)
            .padding(Insets::symmetric(0.0, 4.0))
            .child(super::quick_terminal::toggle_button(state, theme))
    }))
}
