//! LAN access (ADR 0041) in the views: Settings' section on the host, and the connect
//! dialog's pairing code and paired hosts on a device.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, toggle};
use urushi_ui::{Font, Insets, Length, Node, Role, Theme, key};

use super::dialogs::{cancel, field, hint, label};
use crate::msg::{LanMsg, Msg};
use crate::state::{AppState, Dialog, Field, SettingToggle};
use crate::update::lan::offered_hosts;
use crate::view::modal::modal;

/// `address:port`, bracketing IPv6 addresses.
fn with_port(address: &str, port: u32) -> String {
    if address.contains(':') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    }
}

/// One line on LAN access for Settings.
fn status_line(lan: &v1::LanStatus) -> String {
    if !lan.enabled {
        "Off. Nothing listens on your LAN.".to_owned()
    } else if !lan.error.is_empty() {
        lan.error.clone()
    } else if lan.serving {
        let at: Vec<String> = lan
            .addresses
            .iter()
            .map(|a| with_port(a, lan.port))
            .collect();
        format!(
            "Listening at {} — encrypted; only devices you pair can connect",
            at.join(", ")
        )
    } else {
        "Starting…".to_owned()
    }
}

/// Settings' LAN rows: the switch, the status, pairing, and the paired devices.
pub(super) fn settings_rows(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if !state.shows_lan_settings() {
        return Vec::new();
    }
    let lan = state.lan.clone().unwrap_or_default();
    let indent = Insets {
        top: 0.0,
        right: 0.0,
        bottom: 6.0,
        left: 42.0,
    };
    let mut rows = vec![
        toggle(
            theme,
            lan.enabled,
            "Allow LAN access",
            Msg::Toggle(SettingToggle::LanAccess),
        ),
        Node::text(
            status_line(&lan),
            Font::Ui,
            theme.font_size_small,
            if lan.enabled && !lan.error.is_empty() {
                theme.danger
            } else {
                theme.text_muted
            },
        )
        .padding(indent),
    ];
    if !lan.enabled {
        rows.push(
            hint(
                theme,
                "For a home network or a VPN into it (WireGuard, a Dream Machine). Urushi \
                 never opens ports on your router or firewall.",
            )
            .padding(indent),
        );
        return rows;
    }
    rows.push(match &lan.pairing {
        Some(pairing) => pairing_panel(theme, &lan, pairing),
        None => Node::row().padding(indent).child(button(
            theme,
            "Pair a device…",
            ButtonKind::Secondary,
            Msg::Lan(LanMsg::StartPairing),
        )),
    });
    if !lan.devices.is_empty() {
        rows.push(label(theme, "Paired devices"));
        rows.extend(lan.devices.iter().map(|d| device_row(theme, d)));
    }
    rows
}

/// The pairing code, big, with where to type it.
fn pairing_panel(theme: &Theme, lan: &v1::LanStatus, pairing: &v1::LanPairing) -> Node<Msg> {
    let address = lan
        .addresses
        .first()
        .map_or_else(String::new, |a| with_port(a, lan.port));
    Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .padding(Insets::symmetric(10.0, 12.0))
        .radius(theme.radius)
        .background(theme.hover)
        .key(key(&["lan-pairing"]))
        .child(hint(
            theme,
            &format!(
                "On the other device, connect to {address} and enter this code. It works \
                 once, for {} more min.",
                pairing.expires_in_secs.div_ceil(60)
            ),
        ))
        .child(
            Node::text(
                pairing.code.clone(),
                Font::Mono {
                    bold: true,
                    italic: false,
                },
                theme.font_size * 1.4,
                theme.text,
            )
            .role(Role::Label, format!("Pairing code {}", pairing.code)),
        )
        .child(hint(
            theme,
            &format!("This host's key: {}", lan.fingerprint),
        ))
        .child(Node::row().child(button(
            theme,
            "Cancel pairing",
            ButtonKind::Ghost,
            Msg::Lan(LanMsg::CancelPairing),
        )))
}

fn device_row(theme: &Theme, device: &v1::LanDevice) -> Node<Msg> {
    let detail = if device.connected {
        format!("{} · connected", device.fingerprint)
    } else {
        device.fingerprint.clone()
    };
    Node::row()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .align(urushi_ui::Align::Center)
        .key(key(&["lan-device", &device.id]))
        .child(
            Node::column()
                .width(Length::Fill(1.0))
                .gap(2.0)
                .child(Node::text(
                    device.name.clone(),
                    Font::UiMedium,
                    theme.font_size_small,
                    theme.text,
                ))
                .child(hint(theme, &detail)),
        )
        .child(button(
            theme,
            "Revoke",
            ButtonKind::Ghost,
            Msg::Lan(LanMsg::Revoke(device.id.clone())),
        ))
}

/// The connect dialog: a host, an optional pairing code, and the hosts paired before.
/// `None` for any other dialog.
pub(super) fn connect(state: &AppState, theme: &Theme, dialog: &Dialog) -> Option<Node<Msg>> {
    let Dialog::Connect {
        host,
        code,
        busy,
        error,
    } = dialog
    else {
        return None;
    };
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Host"))
        .child(field(
            state,
            theme,
            host,
            Field::Host,
            "machine.tailnet.ts.net or 192.168.1.10",
        ))
        .child(hint(
            theme,
            "A machine serving Urushi on your tailnet (its MagicDNS name or tailnet IP), or \
             the LAN address of one with LAN access on.",
        ))
        .child(label(theme, "Pairing code (LAN, first time only)"))
        .child(field(
            state,
            theme,
            code,
            Field::PairingCode,
            "ABCDE-FGH12-34567-89JKM",
        ))
        .child(hint(
            theme,
            "On the host: Settings → Allow LAN access → Pair a device….",
        ));
    if let Some(error) = error {
        body = body.child(Node::text(
            error,
            Font::Ui,
            theme.font_size_small,
            theme.danger,
        ));
    }
    if !state.lan_hosts.is_empty() {
        body = body.child(label(theme, "Paired hosts"));
        for paired in offered_hosts(&state.lan_hosts) {
            body = body.child(
                Node::row()
                    .width(Length::Fill(1.0))
                    .padding(Insets::symmetric(6.0, 8.0))
                    .gap(10.0)
                    .radius(theme.radius)
                    .hover_background(theme.hover)
                    .key(key(&["lan-host", &paired.address]))
                    .role(Role::Button, format!("Connect to {}", paired.host_name))
                    .on_click(Msg::ConnectTo(paired.address.clone()))
                    .child(
                        Node::text(
                            paired.host_name.clone(),
                            Font::UiMedium,
                            theme.font_size,
                            theme.text,
                        )
                        .width(Length::Fill(1.0)),
                    )
                    .child(Node::text(
                        paired.address.clone(),
                        Font::Ui,
                        theme.font_size_small,
                        theme.text_muted,
                    )),
            );
        }
    }
    let dismiss = (state.target != crate::config::ConnectTarget::Ask).then_some(Msg::CloseDialog);
    let mut actions = Vec::new();
    if dismiss.is_some() {
        actions.push(cancel(theme));
    }
    let main = if *busy { "Pairing…" } else { "Connect" };
    actions.push(button(theme, main, ButtonKind::Primary, Msg::Submit));
    Some(modal(
        state,
        theme,
        "Connect to a host",
        460.0,
        body,
        actions,
        dismiss,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use urushi_client::v1;
    use urushi_ui::{CellMetrics, Point, Rect, TextSystem, Theme, build};

    use crate::config::ConnectTarget;
    use crate::msg::{LanMsg, Msg, PairedHost};
    use crate::state::AppState;

    const CELL: CellMetrics = CellMetrics {
        width: 8.0,
        height: 16.0,
    };

    fn render(state: &AppState) -> urushi_ui::Frame<Msg> {
        build(
            &crate::view::view(state, &Theme::dark(), &HashMap::new()),
            Rect::new(0.0, 0.0, 1200.0, 2400.0),
            &mut TextSystem::bundled_only(),
            None::<Point>,
        )
    }

    fn clicks(frame: &urushi_ui::Frame<Msg>) -> Vec<Msg> {
        frame
            .hits
            .iter()
            .filter_map(|h| h.on_click.clone())
            .collect()
    }

    #[test]
    fn settings_show_the_code_and_the_devices_to_revoke() {
        let mut state = AppState::new(ConnectTarget::Local(None), CELL);
        state.update(Msg::Connected {
            welcome: v1::Welcome {
                protocol_minor: crate::update::lan::LAN_SINCE_MINOR,
                ..Default::default()
            },
            workspace: v1::Workspace::default(),
        });
        state.update(Msg::OpenSettings);
        let dump = render(&state).scene.dump();
        assert!(dump.contains("Allow LAN access"), "{dump}");
        assert!(dump.contains("never opens ports"), "{dump}");

        state.lan = Some(v1::LanStatus {
            enabled: true,
            serving: true,
            addresses: vec!["192.168.1.10".into()],
            port: 7421,
            fingerprint: "0011-2233-4455-6677".into(),
            pairing: Some(v1::LanPairing {
                code: "ABCDE-FGH12-34567-89JKM".into(),
                expires_in_secs: 590,
            }),
            devices: vec![v1::LanDevice {
                id: "d1".into(),
                name: "phone".into(),
                fingerprint: "aaaa-bbbb-cccc-dddd".into(),
                connected: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let frame = render(&state);
        let dump = frame.scene.dump();
        assert!(dump.contains("Listening at 192.168.1.10:7421"), "{dump}");
        assert!(dump.contains("ABCDE-FGH12-34567-89JKM"), "{dump}");
        assert!(dump.contains("for 10 more min"), "{dump}");
        assert!(dump.contains("aaaa-bbbb-cccc-dddd · connected"), "{dump}");
        let clicks = clicks(&frame);
        assert!(
            clicks
                .iter()
                .any(|m| matches!(m, Msg::Lan(LanMsg::Revoke(id)) if id == "d1"))
        );
        assert!(
            clicks
                .iter()
                .any(|m| matches!(m, Msg::Lan(LanMsg::CancelPairing)))
        );
    }

    #[test]
    fn a_device_connecting_never_sees_the_host_section() {
        let mut state = AppState::new(ConnectTarget::Ask, CELL);
        state.update(Msg::LanHosts(vec![PairedHost {
            address: "192.168.1.10:7421".into(),
            host_name: "studio".into(),
        }]));
        let frame = render(&state);
        let dump = frame.scene.dump();
        assert!(dump.contains("Pairing code"), "{dump}");
        assert!(dump.contains("studio"), "{dump}");
        assert!(
            clicks(&frame)
                .iter()
                .any(|m| matches!(m, Msg::ConnectTo(a) if a == "192.168.1.10:7421"))
        );
    }
}
