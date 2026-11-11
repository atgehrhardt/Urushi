//! LAN access (ADR 0040), on both ends:
//! - **Connecting:** the connect dialog pairs with a LAN host first when given a code,
//!   and offers the hosts this device paired with.
//! - **The host's Settings:** switch LAN access on, show a pairing code, revoke devices.
//!   Only shown connected to the daemon on this machine (the daemon refuses these from
//!   anywhere else anyway).

use urushi_client::WorkspaceMirror;
use urushi_client::v1::{self, request::Body, response::Outcome};

use super::request;
use crate::config::ConnectTarget;
use crate::msg::{Effect, Followup, LanMsg, Msg, PairedHost};
use crate::state::{AppState, Connection, Dialog};

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

    /// Drops everything shown and connects to the remote daemon at `address`.
    fn reconnect_to(&mut self, address: String, fx: &mut Vec<Effect>) {
        let target = ConnectTarget::Remote(address);
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

/// The hosts to offer in the connect dialog: paired ones, newest first.
pub(crate) fn offered_hosts(hosts: &[PairedHost]) -> impl Iterator<Item = &PairedHost> {
    hosts.iter().rev()
}
