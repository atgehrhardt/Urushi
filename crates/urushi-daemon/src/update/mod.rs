//! Updating the macOS app from GitHub Releases (ADR 0048).
//!
//! The daemon looks for a newer release ([`github`]) when it starts and every few hours,
//! and tells clients through the workspace (`Workspace.update`). When the user agrees,
//! it downloads the release, checks it and puts it in place of the installed bundle
//! ([`install`]). It doesn't restart anything: the app then restarts the daemon and
//! relaunches itself, and restore points bring the panes back (ADR 0013).
//!
//! Only a daemon running from Urushi's own macOS bundle updates ([`bundle::bundle_of`]),
//! so a development build never touches the installed app. The installed version is
//! read from the bundle on disk, not from this binary: after `make install` the running
//! daemon is older than the app, and the app is what gets updated.

pub mod bundle;
pub mod github;
pub mod install;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use urushi_core::Version;
use urushi_proto::v1;

pub use github::{DEFAULT_FEED, Release};

use crate::http::{CurlHttp, HttpClient};
use crate::service::lock;

/// Where updates come from and what they replace.
pub struct UpdateConfig {
    /// Reaches GitHub.
    pub http: Arc<dyn HttpClient>,
    /// The latest-release endpoint ([`DEFAULT_FEED`]).
    pub feed_url: String,
    /// The installed app bundle to update. `None`: this daemon can't update (not a
    /// macOS app from a release, e.g. a development build).
    pub bundle: Option<PathBuf>,
}

impl UpdateConfig {
    /// The real thing: GitHub over curl, and the bundle this daemon runs from, if any.
    pub fn system() -> Self {
        let bundle = crate::platform::UPDATES_SUPPORTED
            .then(std::env::current_exe)
            .and_then(Result::ok)
            .and_then(|exe| bundle::bundle_of(&exe));
        Self {
            http: Arc::new(CurlHttp::default()),
            feed_url: DEFAULT_FEED.to_owned(),
            bundle,
        }
    }
}

/// What a check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// The installed app's version.
    pub current: Version,
    /// The latest release.
    pub latest: Release,
}

impl Check {
    /// The latest release is newer than the installed app.
    pub fn available(&self) -> bool {
        self.latest.version > self.current
    }
}

/// Why updating isn't possible here at all.
pub const UNSUPPORTED: &str =
    "updates are only available in the macOS app installed from a release";

/// The daemon's knowledge of updates: the newer release last found, and whether one is
/// being installed. In memory only.
pub struct Updater {
    config: UpdateConfig,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The last check that found a newer release.
    available: Option<Check>,
    installing: bool,
}

impl Updater {
    /// An updater with nothing found yet.
    pub fn new(config: UpdateConfig) -> Self {
        Self {
            config,
            state: Mutex::default(),
        }
    }

    /// The bundle this daemon would update, if it can.
    pub fn bundle(&self) -> Option<&Path> {
        self.config.bundle.as_deref()
    }

    /// The installed app's version: its bundle's, or this binary's if that can't be read.
    pub fn installed_version(&self) -> Version {
        self.bundle()
            .and_then(bundle::info)
            .and_then(|info| info.version)
            .or_else(|| env!("CARGO_PKG_VERSION").parse().ok())
            .unwrap_or(Version::new(0, 0, 0))
    }

    /// Asks GitHub for the latest release and remembers it if it's newer. Blocking.
    /// Returns whether what clients see changed, and the check.
    pub fn check(&self) -> Result<(bool, Check), String> {
        if self.bundle().is_none() {
            return Err(UNSUPPORTED.into());
        }
        let arch = github::mac_arch().ok_or("there are no releases for this Mac")?;
        let latest = github::latest(self.config.http.as_ref(), &self.config.feed_url, arch)?;
        let check = Check {
            current: self.installed_version(),
            latest,
        };
        let found = check.available().then(|| check.clone());
        let mut state = lock(&self.state);
        let changed = state.available != found;
        state.available = found;
        Ok((changed, check))
    }

    /// What clients are told: the newer release, while there is one.
    pub fn to_proto(&self) -> Option<v1::AvailableUpdate> {
        let state = lock(&self.state);
        state.available.as_ref().map(|c| v1::AvailableUpdate {
            version: c.latest.version.to_string(),
            current_version: c.current.to_string(),
            release_url: c.latest.page_url.clone(),
            installing: state.installing,
        })
    }

    /// Marks `version` as being installed and returns its release. Refused unless it is
    /// the newer release last found, and while another install runs.
    pub fn begin_install(&self, version: Version) -> Result<Release, String> {
        let mut state = lock(&self.state);
        if state.installing {
            return Err("an update is already being installed".into());
        }
        let release = match &state.available {
            Some(c) if c.latest.version == version => c.latest.clone(),
            Some(c) => {
                return Err(format!(
                    "{version} isn't the latest release any more ({} is)",
                    c.latest.version
                ));
            }
            None => {
                return Err(format!(
                    "no newer release ({version}) is known; check again"
                ));
            }
        };
        state.installing = true;
        Ok(release)
    }

    /// Installs a release begun with [`Updater::begin_install`] into the bundle, using
    /// `work` for the download. Blocking. Returns the bundle.
    pub fn install(&self, release: &Release, work: &Path) -> Result<PathBuf, String> {
        let bundle = self.bundle().ok_or(UNSUPPORTED)?.to_path_buf();
        install::install(self.config.http.as_ref(), release, &bundle, work)
            .map(|()| bundle)
            .map_err(|e| {
                if release.page_url.is_empty() {
                    e
                } else {
                    format!("{e} (release: {})", release.page_url)
                }
            })
    }

    /// Ends an install. Once installed, the release is no longer newer than the app.
    pub fn finish_install(&self, installed: bool) {
        let mut state = lock(&self.state);
        state.installing = false;
        if installed {
            state.available = None;
        }
    }
}
