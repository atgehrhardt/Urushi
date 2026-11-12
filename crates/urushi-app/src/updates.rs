//! What the app knows about updating itself (ADR 0049), apart from the dialog.
//!
//! The daemon finds new releases and installs them; the newer release it knows of
//! arrives with the workspace (`Workspace.update`). The app offers it in a notice in
//! the window's corner, asks before installing (the restart stops running agents), then
//! restarts the daemon and relaunches itself from the updated bundle.

use urushi_client::v1;

/// Where an update the user agreed to is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdatePhase {
    /// Asking: what updating stops, and the button to go ahead.
    Confirm,
    /// The daemon downloads, checks and installs the release.
    Installing,
    /// Installed at this bundle: the daemon is being restarted, then the app relaunches.
    Restarting(String),
}

/// The confirmation's facts, counted when it opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateDialog {
    /// The release, X.Y.Z.
    pub version: String,
    /// The installed app's version.
    pub current: String,
    /// The release's notes.
    pub release_url: String,
    /// Agents working right now, which updating interrupts.
    pub agents: usize,
    /// Open terminals that close while the daemon restarts.
    pub terminals: usize,
    /// Threads they are in.
    pub threads: usize,
    /// How far it got.
    pub phase: UpdatePhase,
}

/// The Updates section of Settings: a check the user asked for, and its answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateChecks {
    /// A check is in flight.
    pub checking: bool,
    /// What the last check found, or why it couldn't look.
    pub last: Option<Result<v1::UpdateCheck, String>>,
}

/// Whether this build of the app can be updated in place (the macOS app).
pub const SUPPORTED: bool = crate::platform::HAS_APP_UPDATES;
