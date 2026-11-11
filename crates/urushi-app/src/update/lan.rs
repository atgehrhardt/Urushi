//! LAN access (ADR 0041), on both ends:
//! - **Connecting:** the connect dialog pairs with a LAN host first when given a code,
//!   and offers the hosts this device connected to or paired with. A phone goes back to
//!   the host it used last, and asks only if that fails (ADR 0043).
//! - **The host's Settings:** switch LAN access on, show a pairing code, revoke devices.
//!   Only shown connected to the daemon on this machine (the daemon refuses these from
//!   anywhere else anyway).

use urushi_client::WorkspaceMirror;
use urushi_client::v1::{self, request::Body, response::Outcome};

use super::request;
use crate::config::ConnectTarget;
use crate::msg::{Effect, Followup, LanMsg, Msg, PairedHost};
use crate::state::{AppState, Connection, Dialog, Field, Resume};

/// The protocol minor version that brought LAN access.
pub(crate) const LAN_SINCE_MINOR: u32 = 26;

impl AppState {
    /// The connection and pairing messages.
    pub(super) fn lan_connect_msg(&mut self, msg: Msg, fx: &mut Vec<Effect>) {
        match msg {
            Msg::ConnectTo(address) | Msg::LanPaired(Ok(address)) => {
                self.reconnect_to(address, fx);
            }
            Msg::LanHosts(hosts) => self.lan_hosts = hosts,
            Msg::RecentHosts(hosts) => self.recent_hosts = hosts,
            Msg::LanPaired(Err(reason)) => {
                if let Some(Dialog::Connect { busy, error, .. }) = &mut self.dialog {
                    *busy = false;
                    *error = Some(reason);
                }
            }
            _ => {}
        }
    }

    /// Enter or Connect in the connect dialog: pair first if a code was given.
    pub(super) fn submit_connect(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::Connect {
            host,
            code,
            busy,
            error,
        }) = &mut self.dialog
        else {
            return;
        };
        let host = host.text().trim().to_owned();
        if *busy || host.is_empty() {
            return;
        }
        let code = code.text().trim().to_owned();
        if code.is_empty() {
            self.reconnect_to(host, fx);
        } else {
            *busy = true;
            *error = None;
            fx.push(Effect::PairLan { host, code });
        }
    }

    /// Opens the connect dialog, starting from the remote host in use, if any.
    pub(super) fn open_connect(&mut self) {
        let dialog = match &self.target {
            ConnectTarget::Remote(host) => Dialog::connect_to(host, None),
            _ => Dialog::connect(),
        };
        self.open(dialog, Some(Field::Host));
    }

    /// Going back to the remembered host failed: the connect dialog asks, once, with
    /// that host filled in and why it failed. Nothing has focus, so a phone's keyboard
    /// doesn't cover the hosts to pick from. Retries go on underneath.
    pub(super) fn resume_failed(&mut self, reason: &str) {
        if self.resume != Resume::Trying {
            return;
        }
        self.resume = Resume::Asked;
        if self.dialog.is_none()
            && let ConnectTarget::Remote(host) = &self.target
        {
            let dialog = Dialog::connect_to(host, Some(reason.to_owned()));
            self.open(dialog, None);
        }
    }

    /// Connected: if the dialog asked because the remembered host was unreachable, the
    /// question is moot now (unless a pairing is under way in it).
    pub(super) fn resume_succeeded(&mut self) {
        if self.resume == Resume::Asked
            && matches!(self.dialog, Some(Dialog::Connect { busy: false, .. }))
        {
            self.dialog = None;
            self.focus = None;
        }
        self.resume = Resume::No;
    }

    /// Drops everything shown and connects to the remote daemon at `address`.
    fn reconnect_to(&mut self, address: String, fx: &mut Vec<Effect>) {
        let target = ConnectTarget::Remote(address);
        self.resume = Resume::No;
        self.target = target.clone();
        self.dialog = None;
        self.focus = None;
        self.connection = Connection::Connecting;
        self.workspace = WorkspaceMirror::default();
        self.deletions = crate::deletions::Deletions::default();
        self.selected_thread = None;
        self.lan = None;
        self.reset_panes();
        fx.push(Effect::Reconnect(target));
    }

    /// Whether the Settings dialog offers LAN access: connected to this machine's own
    /// daemon, new enough to have it.
    pub(crate) fn shows_lan_settings(&self) -> bool {
        matches!(self.target, ConnectTarget::Local(_)) && self.daemon_minor >= LAN_SINCE_MINOR
    }

    /// Asks for the LAN status, when the Settings dialog would show it.
    pub(super) fn request_lan_status(&self, fx: &mut Vec<Effect>) {
        if self.shows_lan_settings() {
            fx.push(request(
                Body::LanStatus(v1::LanStatusRequest {}),
                Followup::LanStatus,
            ));
        }
    }

    /// While Settings shows a pairing code, refreshes the status on every tick, so a
    /// device that pairs appears and the code's time left counts down.
    pub(super) fn poll_lan_pairing(&self, fx: &mut Vec<Effect>) {
        let pairing = self.lan.as_ref().is_some_and(|l| l.pairing.is_some());
        if pairing && matches!(self.dialog, Some(Dialog::Settings { .. })) {
            self.request_lan_status(fx);
        }
    }

    /// The daemon answered a LAN request.
    pub(super) fn lan_answered(
        &mut self,
        followup: &Followup,
        outcome: Outcome,
        fx: &mut Vec<Effect>,
    ) {
        match (followup, outcome) {
            (Followup::LanStatus, Outcome::LanStatus(status)) => self.lan = Some(status),
            (Followup::LanSwitched, Outcome::Settings(settings)) => {
                self.settings = Some(settings);
                self.request_lan_status(fx);
            }
            _ => {}
        }
    }

    /// The LAN switch in Settings.
    pub(super) fn toggle_lan(&self, fx: &mut Vec<Effect>) {
        if !self.shows_lan_settings() {
            return;
        }
        let enabled = self.lan.as_ref().is_some_and(|l| l.enabled);
        let patch = v1::SettingsPatch {
            lan_enabled: Some(!enabled),
            ..Default::default()
        };
        fx.push(request(
            Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
            Followup::LanSwitched,
        ));
    }

    /// Pairing and revoking, from Settings.
    pub(super) fn lan_msg(&self, msg: LanMsg, fx: &mut Vec<Effect>) {
        if !self.shows_lan_settings() {
            return;
        }
        let body = match msg {
            LanMsg::StartPairing => Body::StartLanPairing(v1::StartLanPairingRequest {}),
            LanMsg::CancelPairing => Body::CancelLanPairing(v1::CancelLanPairingRequest {}),
            LanMsg::Revoke(device_id) => {
                Body::RevokeLanDevice(v1::RevokeLanDeviceRequest { device_id })
            }
        };
        fx.push(request(body, Followup::LanStatus));
    }
}

/// A host the connect dialog offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OfferedHost {
    /// What to connect to: `host[:port]`.
    pub address: String,
    /// The host's own name, for a paired LAN host.
    pub name: Option<String>,
}

/// The hosts to offer in the connect dialog, each once: those connected to before,
/// most recent first, then paired LAN hosts not connected to yet, newest first.
pub(crate) fn offered_hosts(recent: &[String], paired: &[PairedHost]) -> Vec<OfferedHost> {
    let name_of = |address: &str| {
        paired
            .iter()
            .find(|p| p.address.eq_ignore_ascii_case(address))
            .map(|p| p.host_name.clone())
    };
    let mut offered: Vec<OfferedHost> = recent
        .iter()
        .map(|address| OfferedHost {
            address: address.clone(),
            name: name_of(address),
        })
        .collect();
    for p in paired.iter().rev() {
        if !recent.iter().any(|r| r.eq_ignore_ascii_case(&p.address)) {
            offered.push(OfferedHost {
                address: p.address.clone(),
                name: Some(p.host_name.clone()),
            });
        }
    }
    offered
}
