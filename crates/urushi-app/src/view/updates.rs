//! Updating the app (ADR 0048): the notice in the window's bottom-right corner, the
//! confirmation that says what updating stops, and the Updates rows of Settings.

use urushi_ui::widgets::{ButtonKind, button, overlay, toggle};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme};

use super::dialogs::{hint, label};
use crate::msg::{Msg, UpdateMsg};
use crate::state::AppState;
use crate::updates::{UpdateDialog, UpdatePhase};
use crate::view::modal::modal;

/// The notice's width.
const NOTICE_WIDTH: f32 = 330.0;
/// Space between the notice and the window's bottom edge: above the status bar.
const NOTICE_BOTTOM: f32 = 48.0;
/// Space between the notice and the window's right edge.
const NOTICE_RIGHT: f32 = 24.0;

fn send(msg: UpdateMsg) -> Msg {
    Msg::AppUpdate(msg)
}

fn line(theme: &Theme, text: &str, color: Color) -> Node<Msg> {
    Node::text(text, Font::Ui, theme.font_size, color).width(Length::Fill(1.0))
}

/// The notice offering a new release, pinned to the bottom-right corner over
/// everything but dialogs and menus. Only the card takes clicks.
pub(super) fn notice(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    let release = state.update_notice()?;
    let card = Node::column()
        .width(Length::Fixed(NOTICE_WIDTH))
        .padding(Insets::all(14.0))
        .gap(8.0)
        .child(
            Node::text(
                format!("Urushi {} is available", release.version),
                Font::UiMedium,
                theme.font_size,
                theme.text,
            )
            .role(
                Role::Label,
                format!("Urushi {} is available", release.version),
            ),
        )
        .child(
            Node::text(
                format!(
                    "You have {}. Updating restarts Urushi.",
                    release.current_version
                ),
                Font::Ui,
                theme.font_size_small,
                theme.text_muted,
            )
            .width(Length::Fill(1.0)),
        )
        .child(
            Node::row()
                .width(Length::Fill(1.0))
                .gap(8.0)
                .justify(Align::End)
                .child(button(
                    theme,
                    "Later",
                    ButtonKind::Ghost,
                    send(UpdateMsg::Later),
                ))
                .child(button(
                    theme,
                    "Update…",
                    ButtonKind::Primary,
                    send(UpdateMsg::Open),
                )),
        )
        .opaque();
    Some(
        Node::column()
            .width(Length::Fill(1.0))
            .height(Length::Fill(1.0))
            .padding(Insets {
                top: 0.0,
                right: NOTICE_RIGHT,
                bottom: NOTICE_BOTTOM,
                left: 0.0,
            })
            .justify(Align::End)
            .align(Align::End)
            .child(overlay(theme, card, theme.radius_large)),
    )
}

/// The confirmation. Like restarting the daemon, only the red button goes ahead;
/// Enter doesn't.
pub(super) fn confirm(state: &AppState, theme: &Theme, d: &UpdateDialog) -> Node<Msg> {
    let mut body = Node::column().width(Length::Fill(1.0)).gap(6.0);
    body = body.child(line(
        theme,
        &format!(
            "Urushi {} replaces {}, then Urushi restarts.",
            d.version, d.current
        ),
        theme.text,
    ));
    if d.agents > 0 {
        body = body
            .child(line(
                theme,
                &format!("{} agent(s) working right now will be stopped.", d.agents),
                theme.warning,
            ))
            .child(line(
                theme,
                "They're reopened after the restart; Claude resumes its conversation.",
                theme.text,
            ))
            .child(line(
                theme,
                "An agent stopped mid-task may need to be asked to continue.",
                theme.text,
            ));
    } else {
        body = body.child(line(theme, "No agent is working right now.", theme.text));
    }
    if d.terminals > 0 {
        body = body
            .child(line(
                theme,
                &format!(
                    "{} terminal(s) in {} thread(s) close while the daemon restarts;",
                    d.terminals, d.threads
                ),
                theme.text_muted,
            ))
            .child(line(
                theme,
                "their output, directories and programs come back.",
                theme.text_muted,
            ));
    }
    if !d.release_url.is_empty() {
        body = body.child(Node::row().child(button(
            theme,
            "Release notes",
            ButtonKind::Ghost,
            Msg::OpenUrl(d.release_url.clone()),
        )));
    }
    let busy = match &d.phase {
        UpdatePhase::Confirm => None,
        UpdatePhase::Installing => Some("Downloading…"),
        UpdatePhase::Restarting(_) => Some("Restarting…"),
    };
    let mut go = button(
        theme,
        busy.unwrap_or("Update and restart"),
        ButtonKind::Danger,
        send(UpdateMsg::Confirm),
    );
    let mut actions = Vec::new();
    if busy.is_some() {
        go.on_click = None;
    } else {
        actions.push(button(
            theme,
            "Cancel",
            ButtonKind::Secondary,
            Msg::CloseDialog,
        ));
    }
    actions.push(go);
    modal(
        state,
        theme,
        &format!("Update Urushi to {}?", d.version),
        540.0,
        body,
        actions,
        busy.is_none().then_some(Msg::CloseDialog),
    )
}

/// The Updates rows of Settings: the automatic-check switch, and checking by hand.
/// Nothing where the app can't update itself (mobile, a remote daemon, an older one).
pub(super) fn settings_rows(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if !state.can_update_app() {
        return Vec::new();
    }
    let checks = &state.update_checks;
    let (status, color) = if checks.checking {
        ("Checking…".to_owned(), theme.text_muted)
    } else if let Some(u) = state.available_update() {
        (
            format!(
                "Urushi {} is available (you have {})",
                u.version, u.current_version
            ),
            theme.text,
        )
    } else {
        match &checks.last {
            Some(Ok(check)) => (
                format!("Urushi {} is up to date", check.current_version),
                theme.text_muted,
            ),
            Some(Err(reason)) => (reason.clone(), theme.warning),
            None => ("New releases come from GitHub".to_owned(), theme.text_muted),
        }
    };
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .align(Align::Center)
        .child(Node::text(status, Font::Ui, theme.font_size, color).width(Length::Fill(1.0)));
    if state.available_update().is_some() {
        row = row.child(button(
            theme,
            "Update…",
            ButtonKind::Primary,
            send(UpdateMsg::Open),
        ));
    } else {
        let mut check = button(
            theme,
            if checks.checking {
                "Checking…"
            } else {
                "Check now"
            },
            ButtonKind::Secondary,
            send(UpdateMsg::CheckNow),
        );
        if checks.checking {
            check.on_click = None;
        }
        row = row.child(check);
    }
    vec![
        label(theme, "Updates"),
        toggle(
            theme,
            state.checks_updates_automatically(),
            "Check for updates automatically",
            send(UpdateMsg::ToggleAutomatic),
        ),
        row,
        hint(
            theme,
            "Updating restarts Urushi; running agents stop and are reopened.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use urushi_client::v1;
    use urushi_ui::{CellMetrics, Point, Rect, TextSystem, Theme, build};

    use crate::config::ConnectTarget;
    use crate::msg::{Msg, UpdateMsg};
    use crate::state::AppState;

    fn render(state: &AppState) -> urushi_ui::Frame<Msg> {
        build(
            &crate::view::view(state, &Theme::dark(), &HashMap::new()),
            Rect::new(0.0, 0.0, 1200.0, 1600.0),
            &mut TextSystem::bundled_only(),
            None::<Point>,
        )
    }

    fn offered() -> AppState {
        let mut state = AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        );
        state.update(Msg::Connected {
            welcome: v1::Welcome {
                protocol_minor: urushi_client::UPDATES_SINCE_MINOR,
                ..Default::default()
            },
            workspace: v1::Workspace {
                update: Some(v1::AvailableUpdate {
                    version: "0.1.7".into(),
                    current_version: "0.1.6".into(),
                    release_url: "https://github.com/o/r/releases/tag/v0.1.7".into(),
                    installing: false,
                }),
                ..Default::default()
            },
        });
        state
    }

    #[test]
    fn the_notice_sits_in_the_bottom_right_corner_and_takes_only_its_own_clicks() {
        let state = offered();
        let frame = render(&state);
        if !crate::updates::SUPPORTED {
            assert!(!frame.scene.dump().contains("is available"));
            return;
        }
        assert!(frame.scene.dump().contains("\"Urushi 0.1.7 is available\""));
        let open = frame
            .hits
            .iter()
            .find(|h| matches!(h.on_click, Some(Msg::AppUpdate(UpdateMsg::Open))))
            .expect("an Update… button");
        assert!(
            open.rect.x > 800.0 && open.rect.y > 1400.0,
            "{:?}",
            open.rect
        );
        assert!(
            frame
                .hits
                .iter()
                .any(|h| matches!(h.on_click, Some(Msg::AppUpdate(UpdateMsg::Later))))
        );
        assert!(
            !matches!(
                frame.click(Point::new(600.0, 400.0)),
                Some(Msg::AppUpdate(_))
            ),
            "the rest of the window isn't covered"
        );
    }

    #[test]
    fn the_confirmation_says_what_stops_and_settings_offer_the_update() {
        let mut state = offered();
        if !crate::updates::SUPPORTED {
            return;
        }
        state.update(Msg::AppUpdate(UpdateMsg::Open));
        let dump = render(&state).scene.dump();
        assert!(dump.contains("Update Urushi to 0.1.7?"), "{dump}");
        assert!(dump.contains("No agent is working right now."));
        assert!(dump.contains("Update and restart"));
        assert!(!dump.contains("is available\""), "the notice steps aside");

        state.update(Msg::CloseDialog);
        state.update(Msg::OpenSettings);
        let frame = render(&state);
        let dump = frame.scene.dump();
        assert!(dump.contains("Check for updates automatically"));
        assert!(dump.contains("Urushi 0.1.7 is available (you have 0.1.6)"));
        assert!(
            frame
                .hits
                .iter()
                .any(|h| matches!(h.on_click, Some(Msg::AppUpdate(UpdateMsg::ToggleAutomatic))))
        );
    }
}
