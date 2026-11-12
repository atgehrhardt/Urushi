//! The workspace service: the daemon's single source of truth (ADR 0002).
//!
//! [`Service`] owns the workspace state (settings, projects, threads, tabs, boards), the
//! live terminal sessions, and every operation on them. It knows nothing about the wire.
//! The `server` module translates protocol messages into calls on this type.
//!
//! # Concurrency
//! State sits behind a `std::sync::Mutex` that is only held for short, synchronous
//! critical sections, never across `.await`. Slow work (git, spawning processes) runs
//! between critical sections and is re-validated before it is committed. Every committed
//! change bumps the revision and wakes subscribers through a `watch` channel at once.
//! Then it waits until the [`saver::Saver`] has written that revision (or a newer one)
//! to disk, so a reply always means the change is saved, while a burst of changes
//! shares one write (ADR 0054).

mod agents;
mod attachments;
mod attention;
mod base_fetch;
mod boards;
mod columns;
mod deletion;
mod fresh_base;
mod jira;
mod jira_create;
mod jira_details;
mod jira_dev;
mod jira_edit;
mod jira_query;
mod jira_sync;
mod jira_write;
mod project_base;
mod projects;
mod quick;
mod quick_filters;
mod repos;
mod restore;
mod running;
mod saver;
mod settings;
mod spawn;
mod state;
mod tabs;
mod task_agent;
mod task_draft;
mod task_tabs;
mod thread_base;
mod threads;
mod timing;
mod updates;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::sync::watch;
use urushi_core::{PaneId, Timestamp};
use urushi_mux::{MuxError, Pty, SessionHandle};
use urushi_proto::v1;

pub use attachments::AttachmentData;
pub use boards::{NewSubtask, NewTask};
pub use deletion::DeletionPlan;
pub use jira::{JiraSignIn, NOT_SIGNED_IN};
pub use jira_dev::{PR_ACTIVE_INTERVAL, PR_IDLE_INTERVAL, PR_PER_ROUND};
pub use jira_edit::IssueChange;
pub use jira_sync::{ACTIVE_INTERVAL, BoardWatch, FULL_INTERVAL, IDLE_INTERVAL, WATCHED_INTERVAL};
pub use project_base::ProjectSettingsRequest;
pub use projects::{Assign, ThreadUpdate};
pub use quick_filters::QuickFilterSaved;
pub use repos::BranchList;
pub use running::is_interrupt;
pub use saver::SaveError;
use spawn::PaneHome;
pub(crate) use state::State;
pub use threads::ThreadRequest;

use crate::agents::AgentLocator;
use crate::git::{Git, GitError};
use crate::lan::LanInterfaces;
use crate::lan::state::LanState;
use crate::restore::{PaneRecord, RestoreStore};
use crate::server::remote::RemoteState;
use crate::store::{Store, StoreError};
use crate::tailscale::TailscaleProvider;
use crate::usage::{Report, UsageMonitor};

/// Why a service operation failed. The server maps each variant to an `ErrorCode`.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// The request was malformed or a value was invalid.
    #[error("{0}")]
    Invalid(String),
    /// A referenced entity doesn't exist.
    #[error("{0} not found")]
    NotFound(String),
    /// The operation isn't allowed in the current state.
    #[error("{0}")]
    Precondition(String),
    /// The request was based on stale state.
    #[error("{0}")]
    Conflict(String),
    /// A git operation failed.
    #[error(transparent)]
    Git(#[from] GitError),
    /// A terminal session couldn't be created.
    #[error(transparent)]
    Mux(#[from] MuxError),
    /// Persisting state failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A change couldn't be confirmed as saved.
    #[error(transparent)]
    Save(#[from] SaveError),
    /// The peer isn't allowed to do this.
    #[error("{0}")]
    Forbidden(String),
    /// A service the daemon relies on (Jira) couldn't be reached or failed, or the daemon
    /// itself is stopping.
    #[error("{0}")]
    Unavailable(String),
    /// An unexpected internal failure.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Result alias for service operations.
pub type ServiceResult<T> = Result<T, ServiceError>;

/// Everything the service needs from the outside world. Each piece is a trait, so tests
/// can substitute fakes.
pub struct ServiceConfig {
    /// Daemon home (app data dir).
    pub home: PathBuf,
    /// Durable storage.
    pub store: Arc<dyn Store>,
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
    /// Which agent runs in a pane, and its plan usage.
    pub usage: Arc<UsageMonitor>,
    /// Jira, for boards that follow it.
    pub jira: Arc<dyn crate::jira::JiraApi>,
    /// The `urushi` CLI next to the daemon, if found.
    pub cli: Option<PathBuf>,
    /// Where app updates come from and which bundle they replace (ADR 0049).
    pub updates: crate::update::UpdateConfig,
}

/// The workspace service. Always used as `Arc<Service>`.
pub struct Service {
    /// Shared with the saver, which snapshots it when a save starts.
    state: Arc<Mutex<State>>,
    sessions: Mutex<HashMap<PaneId, SessionHandle>>,
    /// Writes the workspace document, coalescing bursts of changes (`saver.rs`).
    saver: saver::Saver,
    git: Arc<dyn Git>,
    /// Fetches of new threads' bases, shared and briefly reused (ADR 0055).
    base_fetches: Arc<base_fetch::BaseFetches>,
    pty: Arc<dyn Pty>,
    pub(crate) tailscale: Arc<dyn TailscaleProvider>,
    agents: Arc<AgentLocator>,
    usage: Arc<UsageMonitor>,
    pub(crate) remote: Mutex<RemoteState>,
    pub(crate) lan_interfaces: Arc<dyn LanInterfaces>,
    /// LAN access: key, paired devices, pairing, listeners (ADR 0041).
    pub(crate) lan: Mutex<LanState>,
    /// Restore points on disk (`<home>/restore`).
    restore: RestoreStore,
    /// Attached files on disk (`<home>/attachments`).
    attachments: crate::attachments::AttachmentStore,
    /// The Jira sign-in and how each Jira board's sync is going (`jira.rs`).
    jira: jira::JiraLink,
    /// The last restore point saved for each pane, and the screen version its output
    /// was taken at (so unchanged output isn't read again).
    restore_points: Mutex<HashMap<PaneId, (u64, PaneRecord)>>,
    /// Panes whose program finished or waits for the user. Not stored (`attention.rs`).
    attention: Mutex<HashMap<PaneId, attention::Attention>>,
    /// Panes whose agent is working, since when. Not stored (`running.rs`).
    running: Mutex<HashMap<PaneId, Timestamp>>,
    /// Panes waiting for a task's message to be drafted into their agent, with how many
    /// times the user acted on each since (`task_draft.rs`).
    drafts: Mutex<HashMap<PaneId, u64>>,
    home: PathBuf,
    changes: watch::Sender<u64>,
    /// Woken when a client asks the daemon to stop (`ShutdownDaemon`).
    shutdown: tokio::sync::Notify,
    /// `true` once [`Service::shutdown`] begins. From then on the workspace is frozen: the
    /// panes it kills stay in the layout for the next daemon to restore, work that
    /// outlives the daemon (session watchers) can't change or save it, and connections
    /// close (`Service::stopped`).
    stopping: watch::Sender<bool>,
    me: Weak<Service>,
    /// The `urushi` CLI, for agents in panes (ADR 0024).
    cli: Option<PathBuf>,
    /// App updates: the newer release found, and installing it (`updates.rs`).
    updates: Arc<crate::update::Updater>,
}

/// Locks a mutex, recovering the data if a previous holder panicked. Critical sections
/// never leave the state half-updated, so the data is still consistent.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `f`, which blocks (writing and syncing a file), without stalling the async
/// tasks that share its worker thread: on a multi-threaded runtime they move to another
/// worker while `f` runs. Elsewhere (single-threaded test runtimes, plain threads) `f`
/// just runs. Many panes and agents make changes often, and each one used to hold a
/// worker for a whole `fsync` (UR-69).
pub(crate) fn off_the_runtime<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

/// The current wall-clock time.
pub(crate) fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Timestamp::from_unix_millis(u64::try_from(millis).unwrap_or(u64::MAX))
}

/// A fresh, time-ordered UUID.
pub(crate) fn new_uuid() -> uuid::Uuid {
    uuid::Uuid::now_v7()
}

/// Runs blocking work (git, filesystem) off the async runtime.
pub(crate) async fn blocking<T, F>(f: F) -> ServiceResult<T>
where
    F: FnOnce() -> ServiceResult<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ServiceError::Internal(e.to_string()))?
}

impl Service {
    /// Loads the stored workspace, repairs interrupted operations, and starts every pane
    /// again from its restore point (output, directory, agent). Must be called inside a
    /// tokio runtime.
    pub fn start(config: ServiceConfig) -> ServiceResult<Arc<Self>> {
        let document = config.store.load()?;
        let mut state = State::from_document(document);
        state.repair_interrupted();
        let revision = state.revision;
        let state = Arc::new(Mutex::new(state));
        let snapshot = {
            let state = Arc::clone(&state);
            Box::new(move || {
                let state = lock(&state);
                (state.to_document(), state.revision)
            })
        };
        let saver = saver::Saver::spawn(config.store, snapshot)
            .map_err(|e| ServiceError::Internal(format!("can't start the saver: {e}")))?;
        let service = Arc::new_cyclic(|me| Self {
            state,
            sessions: Mutex::new(HashMap::new()),
            saver,
            git: config.git,
            base_fetches: Arc::new(base_fetch::BaseFetches::new(base_fetch::FRESH_FOR)),
            pty: config.pty,
            tailscale: config.tailscale,
            agents: config.agents,
            usage: config.usage,
            remote: Mutex::new(RemoteState::default()),
            lan_interfaces: config.lan_interfaces,
            lan: Mutex::new(LanState::new(&config.home)),
            restore: RestoreStore::new(config.home.join("restore")),
            attachments: crate::attachments::AttachmentStore::new(config.home.join("attachments")),
            jira: jira::JiraLink::new(config.jira, &config.home),
            restore_points: Mutex::new(HashMap::new()),
            attention: Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            drafts: Mutex::default(),
            home: config.home,
            cli: config.cli,
            updates: Arc::new(crate::update::Updater::new(config.updates)),
            changes: watch::channel(revision).0,
            shutdown: tokio::sync::Notify::new(),
            stopping: watch::channel(false).0,
            me: me.clone(),
        });
        service.restore_sessions();
        service.sweep_attachments();
        let revision = lock(&service.state).revision;
        service.commit(revision)?;
        service.keep_restore_points();
        service.keep_jira_synced();
        service.keep_pull_requests_current();
        service.keep_updates_checked();
        Ok(service)
    }

    /// The daemon's home directory.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Asks the daemon to stop. [`crate::run`] returns once [`Service::shutdown_requested`]
    /// resolves.
    pub fn request_shutdown(&self) {
        self.shutdown.notify_one();
    }

    /// Resolves when a client asked the daemon to stop.
    pub async fn shutdown_requested(&self) {
        self.shutdown.notified().await;
    }

    /// A receiver that wakes on every workspace change (its value is the revision).
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// The whole workspace as a wire message, with each tab's attention and running
    /// panes, and the newer release of the app if one was found.
    pub fn workspace(&self) -> v1::Workspace {
        let state = lock(&self.state);
        let mut workspace = state.to_proto();
        self.add_attention(&mut workspace, &state);
        self.add_running(&mut workspace, &state);
        workspace.update = self.updates.to_proto();
        workspace
    }

    /// Which agent runs in `pane`, and how much of its plan is used.
    pub async fn agent_usage(&self, pane: PaneId) -> ServiceResult<Report> {
        let pid = self.session(pane)?.pid();
        Ok(self.usage.report(pid).await)
    }

    /// The live session of `pane`.
    pub fn session(&self, pane: PaneId) -> ServiceResult<SessionHandle> {
        lock(&self.sessions)
            .get(&pane)
            .cloned()
            .ok_or_else(|| ServiceError::NotFound(format!("pane {pane}")))
    }

    /// Applies `f` to the state, then persists and notifies subscribers. `f` must not
    /// block. If it fails, the change is abandoned. Once the daemon is stopping, every
    /// change is refused.
    pub(crate) fn mutate<R>(
        &self,
        f: impl FnOnce(&mut State) -> ServiceResult<R>,
    ) -> ServiceResult<R> {
        if self.is_stopping() {
            return Err(ServiceError::Unavailable("the daemon is stopping".into()));
        }
        let (result, revision) = {
            let mut state = lock(&self.state);
            let result = f(&mut state)?;
            state.revision += 1;
            (result, state.revision)
        };
        self.commit(revision)?;
        Ok(result)
    }

    /// Whether [`Service::shutdown`] has begun.
    pub(crate) fn is_stopping(&self) -> bool {
        *self.stopping.borrow()
    }

    /// Resolves once [`Service::shutdown`] has begun.
    pub(crate) async fn stopped(&self) {
        let mut stopping = self.stopping.subscribe();
        // The sender lives in `self`, so this only ends when stopping.
        let _ = stopping.wait_for(|stopping| *stopping).await;
    }

    /// Reads the state.
    pub(crate) fn read<R>(&self, f: impl FnOnce(&State) -> ServiceResult<R>) -> ServiceResult<R> {
        f(&lock(&self.state))
    }

    /// Freezes the workspace, then kills every session. Called on shutdown. The kills
    /// aren't recorded as panes exiting, so the stored layout keeps every pane.
    pub fn shutdown(&self) {
        self.stopping.send_replace(true);
        let all: Vec<PaneId> = lock(&self.sessions).keys().copied().collect();
        self.kill_sessions(all);
    }
}
