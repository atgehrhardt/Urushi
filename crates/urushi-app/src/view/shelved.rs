//! A shelved thread (UR-75, ADR 0059): what the main area shows while it is selected,
//! with the button that unshelves it, and the question asked before shelving a thread
//! whose agent is working.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, icon_button, wrapped_text};
use urushi_ui::{Align, Font, Insets, Length, Node, Role, TextSystem, Theme, key};

use super::modal::modal;
use crate::msg::{Msg, ThreadMsg};
use crate::state::AppState;

/// The main area for a shelved thread: what shelving kept, and Unshelve. Opening a
/// shelved thread never starts its terminals by itself.
pub(super) fn view(state: &AppState, theme: &Theme, thread: &v1::Thread) -> Node<Msg> {
    let unshelving = state.shelving.get(&thread.id) == Some(&false);
    let mut unshelve = button(
        theme,
        if unshelving {
            "Unshelving…"
        } else {
            "Unshelve"
        },
        ButtonKind::Primary,
        Msg::Thread(ThreadMsg::Unshelve(thread.id.clone())),
    );
    if unshelving || !state.daemon_supports_shelving() {
        unshelve.on_click = None;
    }
    let card = Node::column()
        .width(if state.compact {
            Length::Fill(1.0)
        } else {
            Length::Fixed(440.0)
        })
        .gap(10.0)
        .child(Node::text(
            format!("\u{201c}{}\u{201d} is shelved", thread.title),
            Font::UiSemibold,
            theme.font_size + 4.0,
            theme.text,
        ))
        .child(
            wrapped_text(
                "Its terminals are stopped. Its worktree, branch and tabs are kept. \
                 Unshelving starts each pane again where it was, and agents resume \
                 their conversations.",
                Font::Ui,
                theme.font_size,
                theme.text_muted,
            )
            .height(Length::Fixed(
                3.0 * TextSystem::line_height(theme.font_size),
            )),
        )
        .child(Node::new().height(Length::Fixed(4.0)))
        .child(Node::row().child(unshelve));
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .children(header(state, theme))
        .child(
            Node::column()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .padding(Insets::all(16.0))
                .justify(Align::Center)
                .align(Align::Center)
                .key(key(&["shelved-thread", &thread.id]))
                .role(Role::Label, format!("Thread {} is shelved", thread.title))
                .child(card),
        )
}

/// Without a tab bar: the way back to the threads on phones, or the button that shows
/// a collapsed sidebar.
fn header(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    let button = if state.compact {
        icon_button(theme, "‹", "Back to threads", Msg::ShowSidebar)
    } else if state.sidebar.collapsed() {
        super::sidebar_toggle(state, theme)
    } else {
        return None;
    };
    Some(
        Node::row()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(40.0))
            .padding(Insets::symmetric(0.0, 8.0))
            .align(Align::Center)
            .child(button),
    )
}

/// Asked before shelving a thread whose agent is working: shelving stops it.
pub(super) fn confirm(state: &AppState, theme: &Theme, thread: &str) -> Node<Msg> {
    let title = state
        .workspace
        .thread(thread)
        .map_or("", |t| t.title.as_str());
    let line = |text: &str, color| {
        Node::text(text, Font::Ui, theme.font_size, color).width(Length::Fill(1.0))
    };
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(line(
            "An agent is working in this thread. Shelving stops it.",
            theme.warning,
        ))
        .child(line(
            "When you unshelve the thread, the agent resumes its conversation.",
            theme.text_muted,
        ));
    modal(
        state,
        theme,
        &format!("Shelve \u{201c}{title}\u{201d}?"),
        420.0,
        body,
        vec![
            button(theme, "Cancel", ButtonKind::Secondary, Msg::CloseDialog),
            button(
                theme,
                "Shelve",
                ButtonKind::Primary,
                Msg::Thread(ThreadMsg::Shelve {
                    thread: thread.to_owned(),
                    confirmed: true,
                }),
            ),
        ],
        Some(Msg::CloseDialog),
    )
}
