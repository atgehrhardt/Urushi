//! Remembered hosts (ADR 0043): a phone goes back to the host it used last, and asks
//! again, pre-filled and saying why, when that host can't be reached (UR-54).

use urushi_client::v1;

use super::{CELL, workspace};
use crate::config::ConnectTarget;
use crate::msg::{Effect, Msg, PairedHost};
use crate::state::{AppState, Connection, Dialog};
use crate::update::lan::{OfferedHost, offered_hosts};

fn connect_dialog(s: &AppState) -> Option<(String, Option<String>)> {
    match &s.dialog {
        Some(Dialog::Connect { host, error, .. }) => Some((host.text().to_owned(), error.clone())),
        _ => None,
    }
}

fn connected(s: &mut AppState) {
    s.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: workspace(),
    });
}

#[test]
fn a_remembered_host_is_connected_to_without_asking() {
    let s = AppState::resuming("box.tail.ts.net".into(), CELL);
    assert_eq!(s.target, ConnectTarget::Remote("box.tail.ts.net".into()));
    assert!(s.dialog.is_none());
    assert!(s.focus.is_none(), "no keyboard over the workspace");
}

#[test]
fn a_remembered_host_that_cannot_be_reached_asks_again_saying_why() {
    let mut s = AppState::resuming("box.tail.ts.net".into(), CELL);
    s.update(Msg::Disconnected(
        "cannot connect to box.tail.ts.net".into(),
    ));
    assert_eq!(
        connect_dialog(&s),
        Some((
            "box.tail.ts.net".into(),
            Some("cannot connect to box.tail.ts.net".into())
        ))
    );
    // It still retries underneath: once it gets through, the dialog goes.
    connected(&mut s);
    assert!(s.dialog.is_none());
    assert!(matches!(s.connection, Connection::Connected { .. }));
    // Losing the connection later only reconnects; it doesn't ask.
    s.update(Msg::Disconnected("lost".into()));
    assert!(s.dialog.is_none());
}

#[test]
fn the_dialog_asks_only_once_and_can_be_put_away() {
    let mut s = AppState::resuming("box.tail.ts.net".into(), CELL);
    s.update(Msg::Disconnected("down".into()));
    s.update(Msg::CloseDialog);
    assert!(s.dialog.is_none(), "a remote target can be dismissed");
    s.update(Msg::Disconnected("still down".into()));
    assert!(s.dialog.is_none(), "asked once");
}

#[test]
fn a_host_chosen_in_the_dialog_is_not_asked_about_again() {
    let mut s = AppState::new(ConnectTarget::Ask, CELL);
    let fx = s.update(Msg::ConnectTo("box.tail.ts.net".into()));
    assert!(matches!(
        &fx[..],
        [Effect::Reconnect(ConnectTarget::Remote(_))]
    ));
    s.update(Msg::Disconnected("down".into()));
    assert!(s.dialog.is_none());
}

#[test]
fn the_connect_dialog_starts_from_the_current_host() {
    let mut s = AppState::resuming("box.tail.ts.net".into(), CELL);
    connected(&mut s);
    s.update(Msg::OpenConnect);
    assert_eq!(connect_dialog(&s), Some(("box.tail.ts.net".into(), None)));
    // With no host chosen yet, it is empty.
    let s = AppState::new(ConnectTarget::Ask, CELL);
    assert_eq!(connect_dialog(&s), Some((String::new(), None)));
}

#[test]
fn recent_and_paired_hosts_are_offered_once_most_recent_first() {
    let mut s = AppState::new(ConnectTarget::Ask, CELL);
    s.update(Msg::RecentHosts(vec![
        "box.tail.ts.net".into(),
        "192.168.1.10:7421".into(),
    ]));
    s.update(Msg::LanHosts(vec![
        PairedHost {
            address: "192.168.1.20:7421".into(),
            host_name: "studio".into(),
        },
        PairedHost {
            address: "192.168.1.10:7421".into(),
            host_name: "mac".into(),
        },
    ]));
    assert_eq!(
        offered_hosts(&s.recent_hosts, &s.lan_hosts),
        [
            OfferedHost {
                address: "box.tail.ts.net".into(),
                name: None,
            },
            OfferedHost {
                address: "192.168.1.10:7421".into(),
                name: Some("mac".into()),
            },
            OfferedHost {
                address: "192.168.1.20:7421".into(),
                name: Some("studio".into()),
            },
        ]
    );
}
