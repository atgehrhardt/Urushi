//! The daemon section of Settings, and the confirmation before restarting it.

use urushi_ui::widgets::{ButtonKind, button, modal};
use urushi_ui::{Font, Length, Node, Theme};

use crate::msg::Msg;
use crate::state::AppState;

/// "Daemon: version · protocol", with a Restart button for a local daemon. When the
/// daemon is older than the app, says what that costs and what restarting does.
pub(super) fn settings_section(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if !state.can_restart_daemon() {
        return Vec::new();
    }
    let outdated = state.daemon_outdated();
    let summary = format!(
        "Daemon {} · protocol 1.{}{}",
        state.daemon_version,
        state.daemon_minor,
        if outdated {
            " (older than this app)"
        } else {
            ""
        }
    );
    let restart = button(
        theme,
        "Restart daemon…",
        if outdated {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        },
        Msg::OpenRestartDaemon,
    );
    let mut out = vec![
        Node::row()
            .width(Length::Fill(1.0))
            .gap(8.0)
            .child(
                Node::text(
                    summary,
                    Font::Ui,
                    theme.font_size,
                    if outdated { theme.warning } else { theme.text },
                )
                .width(Length::Fill(1.0)),
            )
            .child(restart),
    ];
    if outdated {
        // Labels don't wrap, so the explanation is a few short lines.
        out.extend(
            [
                "Agent settings, projects and mouse support need the new daemon.",
                "Installing never replaces a running daemon, because it runs your terminals.",
            ]
            .map(|line| {
                Node::text(line, Font::Ui, theme.font_size_small, theme.text_muted)
                    .width(Length::Fill(1.0))
            }),
        );
    }
    out
}

/// The confirmation. Like deleting a thread, only the red button restarts; Enter doesn't.
pub(super) fn confirm(
    theme: &Theme,
    terminals: usize,
    threads: usize,
    restarting: bool,
) -> Node<Msg> {
    let line = |text: String, color| {
        Node::text(text, Font::Ui, theme.font_size, color).width(Length::Fill(1.0))
    };
    let mut body = Node::column().width(Length::Fill(1.0)).gap(6.0);
    if terminals > 0 {
        body = body
            .child(line(
                format!("{terminals} running terminal(s) in {threads} thread(s) will close."),
                theme.warning,
            ))
            .child(line(
                "Whatever runs in them (agents, servers, builds) stops.".to_owned(),
                theme.text,
            ));
    } else {
        body = body.child(line("No terminals are open.".to_owned(), theme.text));
    }
    body = body.child(line(
        "Threads, tabs and layouts come back with fresh shells.".to_owned(),
        theme.text_muted,
    ));
    let mut restart = button(
        theme,
        if restarting {
            "Restarting…"
        } else {
            "Restart daemon"
        },
        ButtonKind::Danger,
        Msg::ConfirmRestartDaemon,
    );
    let mut actions = Vec::new();
    if restarting {
        restart.on_click = None;
    } else {
        actions.push(button(
            theme,
            "Cancel",
            ButtonKind::Secondary,
            Msg::CloseDialog,
        ));
    }
    actions.push(restart);
    modal(
        theme,
        "Restart the Urushi daemon?",
        520.0,
        body,
        actions,
        (!restarting).then_some(Msg::CloseDialog),
    )
}
