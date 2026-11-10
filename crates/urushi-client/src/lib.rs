//! Client library for `urushid`.
//!
//! - [`Client`]: one connection, with the handshake done. Typed request methods are in
//!   `requests.rs`, and unsolicited events arrive on the returned [`Events`] stream.
//! - [`ScreenState`]: a client-side copy of a pane's screen, kept current by applying
//!   `ScreenUpdate`s.
//! - [`WorkspaceMirror`]: the workspace as last reported, with domain-typed lookups.
//! - [`ensure_local_daemon`]: connects to the local daemon, starting it if needed.

mod boards;
mod connection;
mod jira;
mod jira_issue;
mod launcher;
mod platform;
mod projects;
mod quick;
mod quick_filters;
mod requests;
mod screen;
mod tasks;
mod workspace;

pub use connection::{Client, Events};
pub use launcher::{
    DAEMON_BIN_ENV, StopMethod, StoppedDaemon, ensure_local_daemon, local_daemon_endpoint,
    restart_local_daemon, stop_local_daemon,
};
pub use quick::QUICK_TERMINAL_SINCE_MINOR;
pub use quick_filters::{QUICK_FILTERS_SINCE_MINOR, jira_filter, kept_filter};
pub use screen::{Cell, ScreenState};
pub use tasks::{FoundTask, TaskRef};
pub use urushi_proto::endpoint::{Endpoint, parse_tcp};
pub use urushi_proto::v1;
pub use workspace::WorkspaceMirror;

use thiserror::Error;

/// Client failures.
#[derive(Debug, Error)]
pub enum ClientError {
    /// Couldn't reach the daemon.
    #[error("cannot connect to {endpoint}: {source}")]
    Connect {
        /// Where we tried.
        endpoint: String,
        /// Why it failed.
        source: std::io::Error,
    },
    /// The connection dropped.
    #[error("disconnected from the daemon")]
    Disconnected,
    /// The daemon answered with an error.
    #[error("{message}")]
    Remote {
        /// Machine-readable error kind.
        code: v1::ErrorCode,
        /// Human-readable explanation.
        message: String,
    },
    /// The daemon sent something we didn't expect.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// No local daemon could be started.
    #[error("cannot start the daemon: {0}")]
    Launch(String),
    /// Framing or I/O failure.
    #[error(transparent)]
    Frame(#[from] urushi_proto::codec::FrameError),
}

impl ClientError {
    pub(crate) fn from_wire(error: &v1::Error) -> Self {
        Self::Remote {
            code: v1::ErrorCode::try_from(error.code).unwrap_or(v1::ErrorCode::Unspecified),
            message: error.message.clone(),
        }
    }

    /// The daemon's error code, if this is a daemon-reported error.
    pub fn code(&self) -> Option<v1::ErrorCode> {
        match self {
            Self::Remote { code, .. } => Some(*code),
            _ => None,
        }
    }
}
