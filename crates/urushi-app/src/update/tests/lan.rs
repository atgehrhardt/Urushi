//! LAN access in the app (ADR 0040): pairing from the connect dialog, connecting to
//! paired hosts, and the host's switch, pairing code and revoking in Settings.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::{CELL, bodies, workspace};
use crate::config::ConnectTarget;
use crate::msg::{Effect, Followup, LanMsg, Msg, PairedHost};
use crate::state::{AppState, Dialog, Field, SettingToggle};

/// Connected to the daemon on this machine, new enough for LAN access.
fn host() -> AppState {
    let mut s = AppState::new(ConnectTarget::Local(None), CELL);
    s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_minor: crate::update::lan::LAN_SINCE_MINOR,
            ..Default::default()
        },
        workspace: workspace(),
    });
    s
}

fn lan_status(fx: &[Effect]) -> usize {
    bodies(fx)
        .iter()
        .filter(|b| matches!(b, Body::LanStatus(_)))
        .count()
}

#[test]
fn a_code_pairs_first_and_a_paired_host_is_then_connected() {
    let mut phone = AppState::new(ConnectTarget::Ask, CELL);
    phone.update(Msg::Text("192.168.1.10".into()));
    phone.update(Msg::FocusField(Field::PairingCode));
    phone.update(Msg::Text("abcde-fgh12-34567-89jkm".into()));
    let fx = phone.update(Msg::Submit);
    assert!(
        matches!(&fx[..], [Effect::PairLan { host, code }]
            if host == "192.168.1.10" && code == "abcde-fgh12-34567-89jkm"),
        "{fx:?}"
    );
    assert!(matches!(
        phone.dialog,
        Some(Dialog::Connect { busy: true, .. })
    ));
    assert!(
        phone.update(Msg::Submit).is_empty(),
        "one pairing at a time"
    );

    let fx = phone.update(Msg::LanPaired(Ok("192.168.1.10:7421".into())));
    assert!(
        matches!(&fx[..], [Effect::Reconnect(ConnectTarget::Remote(h))] if h == "192.168.1.10:7421")
    );
    assert!(phone.dialog.is_none());
    assert_eq!(
        phone.target,
        ConnectTarget::Remote("192.168.1.10:7421".into())
    );
}

#[test]
fn a_failed_pairing_stays_in_the_dialog_and_says_why() {
    let mut phone = AppState::new(ConnectTarget::Ask, CELL);
    phone.update(Msg::Text("192.168.1.10".into()));
    phone.update(Msg::FocusField(Field::PairingCode));
    phone.update(Msg::Text("ABCDE-FGH12-34567-89JKM".into()));
    phone.update(Msg::Submit);
    let fx = phone.update(Msg::LanPaired(Err("the code is wrong".into())));
    assert!(fx.is_empty());
    let Some(Dialog::Connect { busy, error, .. }) = &phone.dialog else {
        panic!("the dialog closed");
    };
    assert!(!busy);
    assert_eq!(error.as_deref(), Some("the code is wrong"));
}

#[test]
fn paired_hosts_connect_with_one_click() {
    let mut phone = AppState::new(ConnectTarget::Ask, CELL);
    phone.update(Msg::LanHosts(vec![PairedHost {
        address: "192.168.1.10:7421".into(),
        host_name: "studio".into(),
    }]));
    assert_eq!(phone.lan_hosts.len(), 1);
    let fx = phone.update(Msg::ConnectTo("192.168.1.10:7421".into()));
    assert!(matches!(
        &fx[..],
        [Effect::Reconnect(ConnectTarget::Remote(_))]
    ));
}

#[test]
fn settings_ask_for_lan_status_only_on_the_host() {
    let mut s = host();
    assert_eq!(lan_status(&s.update(Msg::OpenSettings)), 1);

    let mut old = super::connected(); // protocol 1.0
    assert_eq!(lan_status(&old.update(Msg::OpenSettings)), 0);

    let mut remote = AppState::new(ConnectTarget::Remote("box".into()), CELL);
    remote.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_minor: crate::update::lan::LAN_SINCE_MINOR,
            ..Default::default()
        },
        workspace: workspace(),
    });
    assert_eq!(lan_status(&remote.update(Msg::OpenSettings)), 0);
    assert!(
        remote
            .update(Msg::Toggle(SettingToggle::LanAccess))
            .is_empty()
    );
    assert!(remote.update(Msg::Lan(LanMsg::StartPairing)).is_empty());
}

#[test]
fn the_switch_pairing_and_revoking_go_to_the_daemon() {
    let mut s = host();
    s.update(Msg::OpenSettings);
    let fx = s.update(Msg::Toggle(SettingToggle::LanAccess));
    assert!(matches!(&bodies(&fx)[..],
        [Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(p) })] if p.lan_enabled == Some(true)));
    // The status is asked for only once the switch is done.
    let fx = s.update(Msg::Response {
        followup: Followup::LanSwitched,
        result: Ok(Outcome::Settings(v1::Settings::default())),
    });
    assert_eq!(lan_status(&fx), 1);

    let fx = s.update(Msg::Lan(LanMsg::StartPairing));
    assert!(matches!(bodies(&fx)[..], [Body::StartLanPairing(_)]));
    let fx = s.update(Msg::Lan(LanMsg::Revoke("d1".into())));
    assert!(matches!(bodies(&fx)[..], [Body::RevokeLanDevice(r)] if r.device_id == "d1"));

    // While a code shows, every tick refreshes the status; otherwise ticks don't.
    assert_eq!(lan_status(&s.update(Msg::Tick(1))), 0);
    s.update(Msg::Response {
        followup: Followup::LanStatus,
        result: Ok(Outcome::LanStatus(v1::LanStatus {
            enabled: true,
            pairing: Some(v1::LanPairing {
                code: "ABCDE-FGH12-34567-89JKM".into(),
                expires_in_secs: 600,
            }),
            ..Default::default()
        })),
    });
    assert_eq!(lan_status(&s.update(Msg::Tick(2))), 1);
    s.update(Msg::CloseDialog);
    assert_eq!(lan_status(&s.update(Msg::Tick(3))), 0);
}
