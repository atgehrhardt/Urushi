//! App updates (ADR 0049): asking the daemon for a newer release, and having it install
//! one. Restarting the daemon and the app afterwards is the caller's job
//! ([`crate::restart_local_daemon`] and relaunching).

use urushi_proto::v1::{self, request::Body, response::Outcome};

use crate::requests::unexpected;
use crate::{Client, ClientError};

/// The first protocol minor version with updates.
pub const UPDATES_SINCE_MINOR: u32 = 28;

impl Client {
    /// Asks GitHub, through the daemon, for the latest release. Needs protocol 1.28.
    pub async fn check_for_updates(&self) -> Result<v1::UpdateCheck, ClientError> {
        let body = Body::CheckForUpdates(v1::CheckForUpdatesRequest {});
        match self.request(body).await? {
            Outcome::UpdateCheck(check) => Ok(check),
            _ => Err(unexpected("CheckForUpdates")),
        }
    }

    /// Has the daemon download `version` and put it in place of the installed app.
    /// Only from the daemon's own machine. Returns where the new app is.
    pub async fn install_update(&self, version: &str) -> Result<v1::UpdateInstalled, ClientError> {
        let body = Body::InstallUpdate(v1::InstallUpdateRequest {
            version: version.to_owned(),
        });
        match self.request(body).await? {
            Outcome::UpdateInstalled(installed) => Ok(installed),
            _ => Err(unexpected("InstallUpdate")),
        }
    }
}
