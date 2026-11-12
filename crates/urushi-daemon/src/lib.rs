//! `urushid`, Urushi's background service.
//!
//! The daemon is the single source of truth (ADR 0002). It owns:
//! - the workspace: projects, threads, tabs and layouts (persisted by [`store`]),
//! - every terminal session (via `urushi-mux`), alive independently of any client,
//! - git worktrees and branches ([`git`]),
//! - finding installed agent CLIs ([`agents`]),
//! - restore points, so panes come back as they were after a restart ([`restore`]),
//! - remote access over Tailscale ([`tailscale`]) and from paired LAN devices ([`lan`]),
//! - boards that follow Jira, and its sign-in ([`jira`], over [`http`]),
//! - updating the macOS app from GitHub Releases ([`update`]).
//!
//! It is a library as well as a binary, so tests (and future embedders) can run a daemon
//! in-process with fakes injected through [`DaemonOptions`].

pub mod agents;
mod attachments;
pub mod cli;
mod exec;
pub mod git;
pub mod http;
pub mod jira;
pub mod lan;
mod platform;
pub mod restore;
mod server;
pub mod service;
pub mod store;
pub mod tailscale;
mod time;
pub mod update;
pub mod usage;

use std::fs::{File, TryLockError};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;
use urushi_mux::{NativePty, Pty};
use urushi_proto::endpoint::{LocalEndpoint, local_endpoint};

use crate::agents::AgentLocator;
use crate::git::{CliGit, Git};
use crate::http::CurlHttp;
use crate::jira::{CloudJira, JiraApi};
use crate::lan::{LanInterfaces, SystemInterfaces};
use crate::service::{Service, ServiceConfig, ServiceError};
use crate::store::JsonFileStore;
use crate::tailscale::{CliTailscale, TailscaleProvider};
use crate::update::UpdateConfig;
use crate::usage::{SystemUsage, UsageMonitor, UsageSource};

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
    /// This machine's private addresses, for LAN access.
    pub lan_interfaces: Arc<dyn LanInterfaces>,
    /// Finds installed agent CLIs.
    pub agents: Arc<AgentLocator>,
    /// Lists processes and reads agents' plan usage.
    pub usage: Arc<dyn UsageSource>,
    /// Jira, for boards that follow it.
    pub jira: Arc<dyn JiraApi>,
    /// The `urushi` CLI, for agents in panes (`None`: not found; prompts then copy
    /// tasks instead of referring to them).
    pub cli: Option<PathBuf>,
    /// Where app updates come from and which bundle they replace.
    pub updates: UpdateConfig,
}

impl DaemonOptions {
    /// Real git, real PTYs, the installed Tailscale, and agents found on this machine.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            git: Arc::new(CliGit::default()),
            pty: Arc::new(NativePty),
            tailscale: Arc::new(CliTailscale::default()),
            lan_interfaces: Arc::new(SystemInterfaces),
            agents: Arc::new(AgentLocator::system()),
            usage: Arc::new(SystemUsage),
            jira: Arc::new(CloudJira::new(Arc::new(CurlHttp::default()))),
            cli: cli::locate(),
            updates: UpdateConfig::system(),
        }
    }
}

/// Longest a shutdown waits for restore points to be saved.
const RESTORE_SAVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Runs a daemon until `shutdown` resolves (or the local listener fails).
pub async fn run(
    mut options: DaemonOptions,
    shutdown: impl Future<Output = ()>,
) -> Result<(), DaemonError> {
    platform::secure_dir(&options.home)?;
    // Listen where clients look: they derive the endpoint from the home as they spell it,
    // and on Windows the pipe's name is a hash of that text, so the canonical spelling
    // (short names expanded, `..` resolved) would be a different pipe. One daemon per
    // home still holds, through the lock file inside it.
    let endpoint = local_endpoint(&options.home);
    // Canonical paths everywhere else, so worktree paths compare equal to what git and shells report.
    options.home = platform::canonicalize(&options.home)?;
    let lock_file = File::create(options.home.join("urushid.lock"))?;
    match lock_file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(DaemonError::AlreadyRunning(options.home)),
        Err(TryLockError::Error(e)) => return Err(e.into()),
    }
    // Lets tooling (scripts/dev.sh) stop exactly this daemon, never a different one.
    let pid_file = options.home.join("urushid.pid");
    std::fs::write(&pid_file, format!("{}\n", std::process::id()))?;
    // Before any pane opens: tens of panes outgrow the default limit (UR-69).
    platform::raise_open_file_limit();

    let store = Arc::new(JsonFileStore::new(options.home.join("workspace.json")));
    let service = Service::start(ServiceConfig {
        home: options.home.clone(),
        store,
        git: options.git,
        pty: options.pty,
        tailscale: options.tailscale,
        lan_interfaces: options.lan_interfaces,
        agents: options.agents,
        usage: Arc::new(UsageMonitor::new(options.usage)),
        jira: options.jira,
        cli: options.cli,
        updates: options.updates,
    })?;

    // Remote listeners come up beside the local socket, not before it: asking
    // Tailscale can take seconds, and clients wait for the socket (UR-69).
    let remote_start = tokio::spawn({
        let service = service.clone();
        async move {
            server::remote::reconcile(&service).await;
            server::lan::reconcile(&service).await;
        }
    });
    let watchdog = server::remote::spawn_watchdog(Arc::downgrade(&service));
    let lan_watchdog = server::lan::spawn_watchdog(Arc::downgrade(&service));

    let result = tokio::select! {
        result = platform::serve_local(service.clone(), &endpoint) => result.map_err(DaemonError::from),
        () = shutdown => Ok(()),
        () = service.shutdown_requested() => Ok(()),
    };

    tracing::info!("shutting down");
    remote_start.abort();
    watchdog.abort();
    lan_watchdog.abort();
    // Save what every pane is doing before its process ends, so the next daemon can
    // bring it back.
    let save = service.save_restore_points();
    if tokio::time::timeout(RESTORE_SAVE_TIMEOUT, save)
        .await
        .is_err()
    {
        tracing::warn!("saving restore points timed out");
    }
    service.shutdown();
    if let LocalEndpoint::UnixSocket(path) = &endpoint {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(&pid_file);
    drop(lock_file);
    result
}

/// Resolves when the OS asks the process to stop (Ctrl-C, SIGTERM).
pub async fn shutdown_signal() {
    platform::shutdown_signal().await;
}
