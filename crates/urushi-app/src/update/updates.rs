//! Updating the app (ADR 0048): the notice, its confirmation, installing, then
//! restarting the daemon and relaunching; and the Updates section of Settings.
//!
//! The flow, once the user confirms:
//! 1. `InstallUpdate` → the daemon downloads, checks and swaps in the new bundle;
//! 2. [`Effect::StopDaemon`] → the daemon saves restore points and stops;
//! 3. [`Effect::Relaunch`] → the app opens the new bundle once it has quit, and quits.
//!    The new app starts the new daemon, which brings the panes back (ADR 0013).
//!
//! A failed install goes back to the confirmation with the reason, and nothing has
//! stopped.

use urushi_client::v1::{self, request::Body, response::Outcome};

use crate::config::ConnectTarget;
use crate::msg::{Effect, Followup, UpdateMsg};
use crate::state::{AppState, Connection, Dialog};
use crate::updates::{UpdateDialog, UpdatePhase};

fn request(body: Body, followup: Followup) -> Effect {
    Effect::Request { body, followup }
}

fn patch(patch: v1::SettingsPatch) -> Effect {
    request(
        Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
        Followup::Settings,
    )
}

impl AppState {
    /// Whether this app can update itself through the daemon it's connected to: the
    /// macOS app, on the daemon it started on this machine, which speaks protocol 1.27.
    pub fn can_update_app(&self) -> bool {
        crate::updates::SUPPORTED
            && matches!(self.target, ConnectTarget::Local(_))
            && matches!(self.connection, Connection::Connected { .. })
            && self.daemon_minor >= urushi_client::UPDATES_SINCE_MINOR
    }

    /// The newer release the daemon found, if this app can install it.
    pub fn available_update(&self) -> Option<&v1::AvailableUpdate> {
        self.workspace
            .raw()
            .update
            .as_ref()
            .filter(|_| self.can_update_app())
    }

    /// The release the corner notice offers: the one available, unless the user said
    /// "Later" to it or it is being installed.
    pub fn update_notice(&self) -> Option<&v1::AvailableUpdate> {
        let skipped = self
            .settings
            .as_ref()
            .and_then(|s| s.updates.as_ref())
            .map(|u| u.skipped_version.as_str());
        self.available_update()
            .filter(|u| !u.installing && skipped != Some(u.version.as_str()))
            .filter(|_| !matches!(self.dialog, Some(Dialog::InstallUpdate(_))))
    }

    /// Whether the daemon looks for releases on its own (the Settings switch).
    pub fn checks_updates_automatically(&self) -> bool {
        self.settings
            .as_ref()
            .and_then(|s| s.updates.as_ref())
            .is_none_or(|u| u.check_automatically.unwrap_or(true))
    }

    pub(super) fn update_msg(&mut self, msg: UpdateMsg, fx: &mut Vec<Effect>) {
        match msg {
            UpdateMsg::Later => {
                if let Some(update) = self.available_update() {
                    fx.push(patch(v1::SettingsPatch {
                        skipped_update_version: Some(update.version.clone()),
                        ..Default::default()
                    }));
                }
            }
            UpdateMsg::Open => self.open_install_update(),
            UpdateMsg::Confirm => self.confirm_install_update(fx),
            UpdateMsg::CheckNow => {
                if self.can_update_app() && !self.update_checks.checking {
                    self.update_checks.checking = true;
                    fx.push(request(
                        Body::CheckForUpdates(v1::CheckForUpdatesRequest {}),
                        Followup::UpdateChecked,
                    ));
                }
            }
            UpdateMsg::ToggleAutomatic => {
                if self.can_update_app() {
                    fx.push(patch(v1::SettingsPatch {
                        check_updates_automatically: Some(!self.checks_updates_automatically()),
                        ..Default::default()
                    }));
                }
            }
        }
    }

    fn open_install_update(&mut self) {
        let Some(update) = self.available_update() else {
            return;
        };
        let (terminals, threads) = self.workspace.open_terminals();
        let dialog = UpdateDialog {
            version: update.version.clone(),
            current: update.current_version.clone(),
            release_url: update.release_url.clone(),
            agents: self.workspace.running_agents(),
            terminals,
            threads,
            phase: UpdatePhase::Confirm,
        };
        self.menu = None;
        self.open(Dialog::InstallUpdate(Box::new(dialog)), None);
    }

    fn confirm_install_update(&mut self, fx: &mut Vec<Effect>) {
        if !self.can_update_app() {
            return;
        }
        if let Some(Dialog::InstallUpdate(dialog)) = &mut self.dialog
            && dialog.phase == UpdatePhase::Confirm
        {
            dialog.phase = UpdatePhase::Installing;
            self.error = None;
            fx.push(request(
                Body::InstallUpdate(v1::InstallUpdateRequest {
                    version: dialog.version.clone(),
                }),
                Followup::UpdateInstalled,
            ));
        }
    }

    /// Whether the open dialog is an update under way, which can't be cancelled.
    pub(super) fn updating(&self) -> bool {
        matches!(&self.dialog, Some(Dialog::InstallUpdate(d)) if d.phase != UpdatePhase::Confirm)
    }

    /// An answer to `CheckForUpdates` or `InstallUpdate`.
    pub(super) fn update_answered(
        &mut self,
        followup: &Followup,
        result: Result<Outcome, String>,
        fx: &mut Vec<Effect>,
    ) {
        match (followup, result) {
            (Followup::UpdateChecked, result) => {
                self.update_checks.checking = false;
                self.update_checks.last = Some(match result {
                    Ok(Outcome::UpdateCheck(check)) => Ok(check),
                    Ok(_) => Err("unexpected answer".into()),
                    Err(message) => Err(message),
                });
            }
            (Followup::UpdateInstalled, Ok(Outcome::UpdateInstalled(installed))) => {
                if let Some(Dialog::InstallUpdate(dialog)) = &mut self.dialog {
                    dialog.phase = UpdatePhase::Restarting(installed.bundle_path);
                    fx.push(Effect::StopDaemon);
                }
            }
            (Followup::UpdateInstalled, result) => {
                if let Some(Dialog::InstallUpdate(dialog)) = &mut self.dialog {
                    dialog.phase = UpdatePhase::Confirm;
                }
                let reason = result.err().unwrap_or_else(|| "unexpected answer".into());
                self.error = Some(format!("Couldn't update Urushi: {reason}"));
            }
            _ => {}
        }
    }

    /// The daemon stopped for an update: relaunch from the new bundle. Reconnecting
    /// too means that if the relaunch fails, this window carries on with the new daemon.
    /// Returns `false` if no update was restarting.
    pub(super) fn update_daemon_stopped(&mut self, fx: &mut Vec<Effect>) -> bool {
        let Some(Dialog::InstallUpdate(dialog)) = &self.dialog else {
            return false;
        };
        let UpdatePhase::Restarting(bundle) = &dialog.phase else {
            return false;
        };
        fx.push(Effect::Relaunch(bundle.clone()));
        true
    }
}
