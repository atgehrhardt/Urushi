//! App updates (ADR 0048): checking for a newer release now and every few hours, and
//! installing it when a client on this machine asks.
//!
//! The [`crate::update::Updater`] does the work; this wires it to settings, the
//! runtime and subscribers. What it finds isn't stored, like attention: subscribers are
//! woken without a new revision.

use std::time::Duration;

use urushi_core::Version;

use super::{Service, ServiceError, ServiceResult, blocking};
use crate::update::{Check, UNSUPPORTED};

/// How long after start the first automatic check waits, so it doesn't compete with
/// restoring panes.
const FIRST_CHECK: Duration = Duration::from_secs(30);

/// How often the daemon looks for a new release.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

impl Service {
    /// Looks for a newer release now, whatever the settings say.
    pub async fn check_for_updates(&self) -> ServiceResult<Check> {
        let updater = self.updates.clone();
        let (changed, check) = blocking(move || {
            updater.check().map_err(|e| {
                if e == UNSUPPORTED {
                    ServiceError::Precondition(e)
                } else {
                    ServiceError::Unavailable(e)
                }
            })
        })
        .await?;
        if changed {
            self.broadcast();
        }
        Ok(check)
    }

    /// Downloads `version` and puts it in place of the installed app. Returns the
    /// bundle, for the caller to relaunch. Nothing is restarted here.
    pub async fn install_update(&self, version: Version) -> ServiceResult<std::path::PathBuf> {
        if self.updates.bundle().is_none() {
            return Err(ServiceError::Precondition(UNSUPPORTED.into()));
        }
        let release = self
            .updates
            .begin_install(version)
            .map_err(ServiceError::Conflict)?;
        self.broadcast();
        tracing::info!(%version, "installing an update");
        let updater = self.updates.clone();
        let work = self.home.join("updates");
        let result = blocking(move || {
            updater
                .install(&release, &work)
                .map_err(ServiceError::Unavailable)
        })
        .await;
        self.updates.finish_install(result.is_ok());
        self.broadcast();
        match &result {
            Ok(bundle) => tracing::info!(%version, bundle = %bundle.display(), "update installed"),
            Err(e) => tracing::warn!(%version, error = %e, "update failed"),
        }
        result
    }

    /// Checks for a new release shortly after start and then every
    /// [`CHECK_INTERVAL`], while the settings allow it. Failures are only logged.
    pub(super) fn keep_updates_checked(&self) {
        if self.updates.bundle().is_none() {
            return;
        }
        let me = self.me.clone();
        tokio::spawn(async move {
            tokio::time::sleep(FIRST_CHECK).await;
            let mut tick = tokio::time::interval(CHECK_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some(service) = me.upgrade() else { break };
                if service.is_stopping() {
                    break;
                }
                if !service.settings().updates.check_automatically {
                    continue;
                }
                match service.check_for_updates().await {
                    Ok(check) if check.available() => {
                        tracing::info!(version = %check.latest.version, "an update is available");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::debug!(error = %e, "checking for updates failed"),
                }
            }
        });
    }
}
