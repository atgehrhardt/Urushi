//! Restarting the local daemon from the app, e.g. after installing a new version.
//!
//! Installing never touches the running daemon (it owns every terminal), so the user
//! restarts it on purpose: a confirmation says how many terminals will close, then the
//! backend stops the daemon and the app reconnects, which starts the daemon bundled with
//! this app. Panes come back from their restore points (ADR 0013). An update (ADR 0049)
//! goes the same way, then relaunches the app.

use crate::config::ConnectTarget;
use crate::msg::Effect;
use crate::state::{AppState, Connection, Dialog};

impl AppState {
    /// Whether this app can restart the daemon it's connected to (only one it started
    /// itself, on this machine).
    pub fn can_restart_daemon(&self) -> bool {
        matches!(self.target, ConnectTarget::Local(_))
            && matches!(self.connection, Connection::Connected { .. })
    }

    pub(super) fn open_restart_daemon(&mut self) {
        if !self.can_restart_daemon() {
            return;
        }
        let (terminals, threads) = self.workspace.open_terminals();
        self.dialog = Some(Dialog::RestartDaemon {
            terminals,
            threads,
            restarting: false,
        });
        self.focus = None;
    }

    pub(super) fn confirm_restart_daemon(&mut self, fx: &mut Vec<Effect>) {
        if let Some(Dialog::RestartDaemon { restarting, .. }) = &mut self.dialog
            && !*restarting
        {
            *restarting = true;
            fx.push(Effect::StopDaemon);
        }
    }

    /// The old daemon is gone (or wouldn't stop): reconnect either way. Connecting
    /// starts this app's daemon if none is running.
    pub(super) fn daemon_stopped(&mut self, result: Result<(), String>, fx: &mut Vec<Effect>) {
        // After an update, the app relaunches from the new bundle (`updates.rs`).
        let updated = self.update_daemon_stopped(fx);
        self.dialog = None;
        self.focus = None;
        if let Err(message) = result {
            self.error = Some(format!("Could not restart the daemon: {message}"));
        } else if updated {
            tracing::info!("the daemon stopped for an update");
        }
        self.connection = Connection::Connecting;
        self.reset_panes();
        fx.push(Effect::Reconnect(self.target.clone()));
    }
}
