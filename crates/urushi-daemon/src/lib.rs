//! `urushid`, Urushi's background service.
//!
//! The daemon is the single source of truth (ADR 0002). It owns:
//! - the workspace: projects, threads, tabs and layouts (persisted by [`store`]),
//! - every terminal session (via `urushi-mux`), alive independently of any client,
//! - git worktrees and branches ([`git`]),
//! - remote access over Tailscale ([`tailscale`]).
//!
//! It is a library as well as a binary, so tests (and future embedders) can run a daemon
//! in-process with fakes injected through [`DaemonOptions`].

pub mod git;
mod platform;
mod server;
pub mod service;
pub mod store;
pub mod tailscale;

use std::fs::{File, TryLockError};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;
use urushi_mux::{NativePty, Pty};
use urushi_proto::endpoint::{LocalEndpoint, local_endpoint};

use crate::git::{CliGit, Git};
use crate::service::{Service, ServiceConfig, ServiceError};
use crate::store::JsonFileStore;
use crate::tailscale::{CliTailscale, TailscaleProvider};

/// Daemon startup failures.
#[derive(Debug, Error)]
pub enum DaemonError {
    /// Another daemon already owns this home directory.
    #[error("another urushid is already running for {0}")]
    AlreadyRunning(PathBuf),
    /// Filesystem or socket failure.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// Loading the workspace failed.
    #[error(transparent)]
    Service(#[from] ServiceError),
}

/// How to run a daemon. [`DaemonOptions::new`] gives the real implementations.
pub struct DaemonOptions {
    /// Home directory: state, socket, worktrees.
    pub home: PathBuf,
    /// Git implementation.
    pub git: Arc<dyn Git>,
    /// PTY implementation.
    pub pty: Arc<dyn Pty>,
    /// Tailscale access.
    pub tailscale: Arc<dyn TailscaleProvider>,
}

impl DaemonOptions {
    /// Real git, real PTYs, and the installed Tailscale.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            git: Arc::new(CliGit::default()),
            pty: Arc::new(NativePty),
            tailscale: Arc::new(CliTailscale::default()),
        }
    }
}

/// Runs a daemon until `shutdown` resolves (or the local listener fails).
pub async fn run(
    mut options: DaemonOptions,
    shutdown: impl Future<Output = ()>,
) -> Result<(), DaemonError> {
    platform::secure_dir(&options.home)?;
    // Canonical paths everywhere, so worktree paths compare equal to what git and shells report.
    options.home = std::fs::canonicalize(&options.home)?;
    let lock_file = File::create(options.home.join("urushid.lock"))?;
    match lock_file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(DaemonError::AlreadyRunning(options.home)),
        Err(TryLockError::Error(e)) => return Err(e.into()),
    }

    let store = Arc::new(JsonFileStore::new(options.home.join("workspace.json")));
    let service = Service::start(ServiceConfig {
        home: options.home.clone(),
        store,
        git: options.git,
        pty: options.pty,
        tailscale: options.tailscale,
    })?;

    server::remote::reconcile(&service).await;
    let watchdog = server::remote::spawn_watchdog(Arc::downgrade(&service));

    let endpoint = local_endpoint(&options.home);
    let result = tokio::select! {
        result = platform::serve_local(service.clone(), &endpoint) => result.map_err(DaemonError::from),
        () = shutdown => Ok(()),
    };

    tracing::info!("shutting down");
    watchdog.abort();
    service.shutdown();
    if let LocalEndpoint::UnixSocket(path) = &endpoint {
        let _ = std::fs::remove_file(path);
    }
    drop(lock_file);
    result
}

/// Resolves when the OS asks the process to stop (Ctrl-C, SIGTERM).
pub async fn shutdown_signal() {
    platform::shutdown_signal().await;
}
