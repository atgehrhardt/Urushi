//! The launcher's choices (terminal, or an installed agent CLI), shown as a picker
//! dialog for a new tab and inline in a thread that has no tabs.

use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Align, Font, Insets, Length, Node, Role, Theme, key};

use crate::msg::Msg;
use crate::state::{AppState, Launch};
use crate::view::modal::modal;

/// One choice: a numbered row that starts it when clicked.
fn choice(
    state: &AppState,
    theme: &Theme,
    index: usize,
    launch: Launch,
    highlighted: bool,
) -> Node<Msg> {
    let detail = match launch {
        Launch::Terminal => "A shell in this thread's worktree".to_owned(),
        Launch::Agent(agent) => agent.command_line(agent.program(), state.skips_permissions(agent)),
    };
    let badge = Node::row()
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(22.0))
        .radius(4.0)
        .background(theme.background)
        .border(1.0, theme.border)
        .justify(Align::Center)
        .child(Node::text(
            (index + 1).to_string(),
            Font::UiMedium,
            theme.font_size_small,
            theme.text_muted,
        ));
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets::symmetric(10.0, 12.0))
        .gap(12.0)
        .radius(theme.radius)
        .background(theme.surface)
        .hover_background(theme.hover)
        .border(
            1.0,
            if highlighted {
                theme.accent
            } else {
                theme.border
            },
        )
        .key(key(&["launch", launch.label()]))
        .on_click(Msg::Launch(launch))
        .role(Role::Button, format!("Open {}", launch.label()))
        .child(badge)
        .child(
            Node::column()
                .width(Length::Fill(1.0))
                .gap(2.0)
                .child(Node::text(
                    launch.label(),
                    Font::UiMedium,
                    theme.font_size,
                    theme.text,
                ))
                .child(Node::text(
                    detail,
                    Font::Ui,
                    theme.font_size_small,
                    theme.text_muted,
                )),
        )
}

/// The list of choices, with `highlighted` marked (if any).
fn choices(state: &AppState, theme: &Theme, highlighted: Option<usize>) -> Node<Msg> {
    Node::column().width(Length::Fill(1.0)).gap(8.0).children(
        state
            .launch_choices()
            .into_iter()
            .enumerate()
            .map(|(i, launch)| choice(state, theme, i, launch, highlighted == Some(i))),
    )
}

fn footnote(state: &AppState, theme: &Theme) -> Node<Msg> {
    let text = if !state.daemon_supports_agents() {
        "Agents appear here once the Urushi daemon is updated (see Settings)."
    } else if state.agents.is_empty() {
        "Claude, Codex and Cursor appear here once their CLIs are installed."
    } else {
        "Agents ask before acting unless you turn that off in Settings."
    };
    Node::text(text, Font::Ui, theme.font_size_small, theme.text_muted)
}

/// The picker dialog for a new tab.
pub(super) fn dialog(state: &AppState, theme: &Theme, highlighted: usize) -> Node<Msg> {
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(12.0)
        .child(choices(state, theme, Some(highlighted)))
        .child(footnote(state, theme));
    modal(
        state,
        theme,
        "New tab",
        420.0,
        body,
        vec![button(
            theme,
            "Cancel",
            ButtonKind::Secondary,
            Msg::CloseDialog,
        )],
        Some(Msg::CloseDialog),
    )
}

/// What a thread with no tabs shows: the choices, centred.
pub(super) fn inline(state: &AppState, theme: &Theme, thread_title: &str) -> Node<Msg> {
    // Phones: use the full width (with a margin) rather than a fixed card.
    let width = if state.compact {
        Length::Fill(1.0)
    } else {
        Length::Fixed(420.0)
    };
    let card = Node::column()
        .width(width)
        .gap(12.0)
        .child(Node::text(
            format!("Start working in “{thread_title}”"),
            Font::UiSemibold,
            theme.font_size + 4.0,
            theme.text,
        ))
        .child(Node::text(
            "Pick what to run. Press a number, or Enter for a terminal.",
            Font::Ui,
            theme.font_size,
            theme.text_muted,
        ))
        .child(choices(state, theme, None))
        .child(footnote(state, theme));
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets::all(16.0))
        .justify(Align::Center)
        .align(Align::Center)
        .child(card)
}
