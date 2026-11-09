//! Restarting the daemon from the app: what the confirmation says, that only the button
//! restarts, and reconnecting afterwards.

use super::{CELL, attached, named};
use crate::config::ConnectTarget;
use crate::keys::Named;
use crate::msg::{Effect, Msg};
use crate::state::{AppState, Connection, Dialog};

#[test]
fn the_confirmation_counts_what_will_close_and_only_the_button_restarts() {
    let mut s = attached();
    s.update(Msg::OpenRestartDaemon);
    assert!(matches!(
        s.dialog,
        Some(Dialog::RestartDaemon {
            terminals: 1,
            threads: 1,
            restarting: false
        })
    ));
    assert!(
        s.update(Msg::Key(named(Named::Enter))).is_empty(),
        "Enter never restarts"
    );

    let fx = s.update(Msg::ConfirmRestartDaemon);
    assert!(matches!(&fx[..], [Effect::StopDaemon]));
    assert!(s.update(Msg::ConfirmRestartDaemon).is_empty(), "only once");
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.dialog.is_some(), "a restart under way can't be cancelled");

    let fx = s.update(Msg::DaemonStopped(Ok(())));
    assert!(s.dialog.is_none());
    assert_eq!(s.connection, Connection::Connecting);
    assert!(s.screens.is_empty(), "old panes are gone");
    assert!(matches!(
        &fx[..],
        [Effect::Reconnect(ConnectTarget::Local(_))]
    ));
}

#[test]
fn a_failed_stop_is_shown_and_the_app_reconnects_anyway() {
    let mut s = attached();
    s.update(Msg::OpenRestartDaemon);
    s.update(Msg::ConfirmRestartDaemon);
    let fx = s.update(Msg::DaemonStopped(Err("did not stop in time".into())));
    assert!(s.error.as_deref().unwrap().contains("did not stop in time"));
    assert!(matches!(&fx[..], [Effect::Reconnect(_)]));
}

#[test]
fn remote_daemons_cant_be_restarted_from_here() {
    let mut s = AppState::new(ConnectTarget::Remote("box".into()), CELL);
    s.connection = Connection::Connected { host: "box".into() };
    s.update(Msg::OpenRestartDaemon);
    assert!(s.dialog.is_none());
}
