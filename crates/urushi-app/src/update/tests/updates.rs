//! Updating the app (ADR 0049): when the notice shows, "Later", what the confirmation
//! counts, and install → stop the daemon → relaunch, or back to asking on failure.
//! Only the macOS app updates itself, so elsewhere these check that nothing is offered.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::{CELL, attached, bodies, named};
use crate::config::ConnectTarget;
use crate::keys::Named;
use crate::msg::{Effect, Followup, Msg, UpdateMsg};
use crate::state::{AppState, Connection, Dialog};
use crate::updates::{SUPPORTED, UpdatePhase};

const BUNDLE: &str = "/Applications/Urushi.app";

fn offer(version: &str) -> v1::AvailableUpdate {
    v1::AvailableUpdate {
        version: version.into(),
        current_version: "0.1.6".into(),
        release_url: format!("https://github.com/o/r/releases/tag/v{version}"),
        installing: false,
    }
}

/// An app attached to a local daemon speaking 1.28 that found `version`.
fn updatable(version: &str) -> AppState {
    let mut s = attached();
    s.daemon_minor = urushi_client::UPDATES_SINCE_MINOR;
    let mut workspace = s.workspace.raw().clone();
    workspace.update = Some(offer(version));
    workspace.tabs[0].running = vec![v1::PaneRunning {
        pane_id: workspace.tabs[0].focused_pane_id.clone(),
        since_ms: 1,
    }];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s
}

fn settings(skipped: &str, automatic: bool) -> Msg {
    Msg::Response {
        followup: Followup::Settings,
        result: Ok(Outcome::Settings(v1::Settings {
            updates: Some(v1::UpdateSettings {
                check_automatically: Some(automatic),
                skipped_version: skipped.into(),
            }),
            ..Default::default()
        })),
    }
}

fn patches(fx: &[Effect]) -> Vec<v1::SettingsPatch> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::UpdateSettings(r) => r.patch.clone(),
            _ => None,
        })
        .collect()
}

#[test]
fn a_newer_release_is_offered_until_the_user_says_later() {
    let mut s = updatable("0.1.7");
    if !SUPPORTED {
        assert!(
            s.update_notice().is_none(),
            "only the macOS app updates itself"
        );
        return;
    }
    assert_eq!(s.update_notice().unwrap().version, "0.1.7");

    let fx = s.update(Msg::AppUpdate(UpdateMsg::Later));
    assert_eq!(
        patches(&fx)[0].skipped_update_version.as_deref(),
        Some("0.1.7")
    );
    s.update(settings("0.1.7", true));
    assert!(s.update_notice().is_none(), "skipped");
    assert!(s.available_update().is_some(), "Settings still offer it");

    let mut newer = s.workspace.raw().clone();
    newer.update = Some(offer("0.1.8"));
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(newer)));
    assert_eq!(
        s.update_notice().unwrap().version,
        "0.1.8",
        "a newer one is offered"
    );
}

#[test]
fn nothing_is_offered_by_an_older_or_a_remote_daemon() {
    let mut s = updatable("0.1.7");
    s.daemon_minor = urushi_client::UPDATES_SINCE_MINOR - 1;
    assert!(s.update_notice().is_none());
    s.update(Msg::AppUpdate(UpdateMsg::Open));
    assert!(s.dialog.is_none());

    let mut remote = AppState::new(ConnectTarget::Remote("box".into()), CELL);
    remote.connection = Connection::Connected { host: "box".into() };
    remote.daemon_minor = urushi_client::UPDATES_SINCE_MINOR;
    let workspace = v1::Workspace {
        update: Some(offer("0.1.7")),
        ..Default::default()
    };
    remote.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    assert!(
        remote.update_notice().is_none(),
        "a phone can't update the desktop's app"
    );
    assert!(
        remote
            .update(Msg::AppUpdate(UpdateMsg::CheckNow))
            .is_empty()
    );
}

#[test]
fn confirming_installs_then_restarts_the_daemon_then_relaunches() {
    let mut s = updatable("0.1.7");
    if !SUPPORTED {
        return;
    }
    s.update(Msg::AppUpdate(UpdateMsg::Open));
    let Some(Dialog::InstallUpdate(dialog)) = &s.dialog else {
        panic!("no confirmation: {:?}", s.dialog);
    };
    assert_eq!((dialog.agents, dialog.terminals, dialog.threads), (1, 1, 1));
    assert_eq!(dialog.phase, UpdatePhase::Confirm);
    assert!(s.update_notice().is_none(), "the notice steps aside");
    assert!(
        s.update(Msg::Key(named(Named::Enter))).is_empty(),
        "Enter never updates"
    );

    let fx = s.update(Msg::AppUpdate(UpdateMsg::Confirm));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::InstallUpdate(v1::InstallUpdateRequest { version })] if version == "0.1.7"
    ));
    assert!(
        s.update(Msg::AppUpdate(UpdateMsg::Confirm)).is_empty(),
        "only once"
    );
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.dialog.is_some(), "an update under way can't be cancelled");

    let fx = s.update(Msg::Response {
        followup: Followup::UpdateInstalled,
        result: Ok(Outcome::UpdateInstalled(v1::UpdateInstalled {
            version: "0.1.7".into(),
            bundle_path: BUNDLE.into(),
        })),
    });
    assert!(matches!(&fx[..], [Effect::StopDaemon]));

    let fx = s.update(Msg::DaemonStopped(Ok(())));
    assert!(s.dialog.is_none());
    assert!(
        matches!(&fx[..], [Effect::Relaunch(b), Effect::Reconnect(_)] if b == BUNDLE),
        "{fx:?}"
    );
}

#[test]
fn a_failed_install_asks_again_with_the_reason_and_stops_nothing() {
    let mut s = updatable("0.1.7");
    if !SUPPORTED {
        return;
    }
    s.update(Msg::AppUpdate(UpdateMsg::Open));
    s.update(Msg::AppUpdate(UpdateMsg::Confirm));
    let fx = s.update(Msg::Response {
        followup: Followup::UpdateInstalled,
        result: Err("the download is damaged".into()),
    });
    assert!(fx.is_empty(), "the daemon keeps running");
    assert!(matches!(
        &s.dialog,
        Some(Dialog::InstallUpdate(d)) if d.phase == UpdatePhase::Confirm
    ));
    assert!(
        s.error
            .as_deref()
            .unwrap()
            .contains("the download is damaged")
    );
    s.update(Msg::CloseDialog);
    assert!(s.dialog.is_none(), "it can be cancelled again");
}

#[test]
fn settings_check_now_and_switch_automatic_checks() {
    let mut s = updatable("0.1.7");
    if !SUPPORTED {
        return;
    }
    let fx = s.update(Msg::AppUpdate(UpdateMsg::CheckNow));
    assert!(matches!(bodies(&fx)[..], [Body::CheckForUpdates(_)]));
    assert!(
        s.update(Msg::AppUpdate(UpdateMsg::CheckNow)).is_empty(),
        "one check at a time"
    );
    s.update(Msg::Response {
        followup: Followup::UpdateChecked,
        result: Err("GitHub is limiting requests".into()),
    });
    assert!(!s.update_checks.checking);
    assert_eq!(
        s.update_checks.last,
        Some(Err("GitHub is limiting requests".into()))
    );
    assert!(s.error.is_none(), "shown in Settings, not as an error");

    assert!(s.checks_updates_automatically(), "on by default");
    let fx = s.update(Msg::AppUpdate(UpdateMsg::ToggleAutomatic));
    assert_eq!(patches(&fx)[0].check_updates_automatically, Some(false));
    s.update(settings("", false));
    let fx = s.update(Msg::AppUpdate(UpdateMsg::ToggleAutomatic));
    assert_eq!(patches(&fx)[0].check_updates_automatically, Some(true));
}
