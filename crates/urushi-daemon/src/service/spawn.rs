//! Starting and ending terminal sessions for panes, in a thread's worktree or, for the
//! quick terminal, in the user's home directory (ADR 0031).

use std::path::PathBuf;

use urushi_core::{PaneId, Thread};
use urushi_mux::{AlacrittyEmulator, SessionHandle, SpawnSpec, TermSize};

use super::{Service, ServiceResult, lock};

/// Where a pane belongs, which sets the directory its shell starts in and the
/// `URUSHI_*` variables it gets.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PaneHome<'a> {
    /// A tab of this thread: starts in its worktree, with `URUSHI_THREAD_ID` and
    /// `URUSHI_WORKTREE`.
    Thread(&'a Thread),
    /// The quick terminal: belongs to no thread, so it starts in the home directory
    /// (the PTY's default) and has no thread variables.
    Quick,
}

/// Variables that describe one pane. A daemon started from inside a pane (a dev daemon,
/// or one the CLI started) inherits that pane's values; new panes must never see them,
/// so they are cleared before each pane's own are set.
const PANE_VARS: [&str; 5] = [
    "URUSHI_THREAD_ID",
    "URUSHI_WORKTREE",
    "URUSHI_PANE_ID",
    urushi_core::agent::hooks::TTY_ENV,
    urushi_core::TASK_PROMPT_ENV,
];

impl<'a> From<&'a Thread> for PaneHome<'a> {
    fn from(thread: &'a Thread) -> Self {
        Self::Thread(thread)
    }
}

impl PaneHome<'_> {
    /// The directory a new shell starts in when no other is asked for. `None` lets the
    /// PTY start in the user's home directory.
    fn default_dir(self) -> Option<PathBuf> {
        match self {
            Self::Thread(t) => t.worktree_path.is_dir().then(|| t.worktree_path.clone()),
            Self::Quick => None,
        }
    }

    /// The variables that say where the pane belongs.
    fn env(self) -> Vec<(String, String)> {
        match self {
            Self::Thread(t) => vec![
                ("URUSHI_THREAD_ID".into(), t.id.to_string()),
                (
                    "URUSHI_WORKTREE".into(),
                    t.worktree_path.to_string_lossy().into_owned(),
                ),
            ],
            Self::Quick => Vec::new(),
        }
    }
}

impl Service {
    /// Starts a terminal session for `pane`, in its home's directory.
    pub(super) fn spawn_session<'a>(
        &self,
        pane: PaneId,
        home: impl Into<PaneHome<'a>>,
        shell: Option<String>,
        size: TermSize,
    ) -> ServiceResult<()> {
        self.spawn_session_with_env(pane, home, shell, size, Vec::new())
    }

    /// Like [`Self::spawn_session`], with extra environment variables for the shell
    /// (e.g. a task's prompt, ADR 0014).
    pub(super) fn spawn_session_with_env<'a>(
        &self,
        pane: PaneId,
        home: impl Into<PaneHome<'a>>,
        shell: Option<String>,
        size: TermSize,
        extra_env: Vec<(String, String)>,
    ) -> ServiceResult<()> {
        let emulator = AlacrittyEmulator::new(size);
        self.spawn_session_in(pane, home, shell, None, emulator, extra_env)
    }

    /// Starts a terminal session for `pane` in `cwd` (when it's still a directory; else
    /// its home's default), with `emulator` (which may already show earlier output) and
    /// `extra_env` on top of the variables every pane gets.
    pub(super) fn spawn_session_in<'a>(
        &self,
        pane: PaneId,
        home: impl Into<PaneHome<'a>>,
        shell: Option<String>,
        cwd: Option<PathBuf>,
        emulator: AlacrittyEmulator,
        extra_env: Vec<(String, String)>,
    ) -> ServiceResult<()> {
        use urushi_mux::Emulator as _;
        let home = home.into();
        let size = emulator.size();
        let cwd = cwd.filter(|d| d.is_dir()).or_else(|| home.default_dir());
        let mut env = home.env();
        env.push(("URUSHI_PANE_ID".into(), pane.to_string()));
        // Agents started here report when they finish or wait (ADR 0017), and can run
        // the CLI against this daemon (ADR 0024).
        env.extend(urushi_core::agent::hooks::hook_env());
        env.extend(crate::cli::pane_env(
            &self.home,
            self.cli.as_deref(),
            std::env::var_os("PATH"),
        ));
        env.extend(extra_env);
        let spec = SpawnSpec {
            program: shell,
            args: Vec::new(),
            cwd,
            env_remove: PANE_VARS.iter().map(|&v| v.to_owned()).collect(),
            env,
            size: Some(size),
            tty_env: Some(urushi_core::agent::hooks::TTY_ENV.into()),
        };
        let handle = SessionHandle::spawn(&*self.pty, &spec, Box::new(emulator))?;
        lock(&self.sessions).insert(pane, handle.clone());
        self.watch_session(pane, handle);
        Ok(())
    }

    /// Raises attention when the pane's program asks for the user, follows whether its
    /// agent runs, and closes the pane (and its tab, if it was the last one) when its
    /// process exits.
    pub(super) fn watch_session(&self, pane: PaneId, handle: SessionHandle) {
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut status = handle.status();
            let (mut alerts, mut activity) = {
                let first = status.borrow();
                (first.alerts, first.activity)
            };
            loop {
                let now = *status.borrow_and_update();
                if now.alerts > alerts {
                    alerts = now.alerts;
                    if let (Some(kind), Some(service)) = (now.alert, me.upgrade()) {
                        service.raise_attention(pane, kind);
                    }
                }
                // After attention, which stops running: `running` says which came last.
                if now.activity > activity {
                    activity = now.activity;
                    if let Some(service) = me.upgrade() {
                        service.set_running(pane, now.running);
                    }
                }
                if now.exit_code.is_some() {
                    break;
                }
                if status.changed().await.is_err() {
                    break;
                }
            }
            drop(handle);
            if let Some(service) = me.upgrade() {
                service.on_pane_exited(pane);
            }
        });
    }

    pub(super) fn on_pane_exited(&self, pane: PaneId) {
        // The daemon killed it on its way out: the pane stays, to be restored.
        if self.is_stopping() {
            return;
        }
        self.forget_attention(&[pane]);
        self.forget_running(&[pane]);
        let removed = lock(&self.sessions).remove(&pane);
        if let Some(session) = removed {
            tokio::spawn(async move {
                let _ = session.kill().await;
            });
        }
        let result = self.mutate(|state| {
            state.remove_pane(pane);
            Ok(())
        });
        if let Err(e) = result {
            tracing::error!(%pane, error = %e, "failed to record pane exit");
        }
    }

    /// Kills the sessions of `panes` and forgets them.
    pub(super) fn kill_sessions(&self, panes: impl IntoIterator<Item = PaneId>) {
        let panes: Vec<PaneId> = panes.into_iter().collect();
        self.forget_attention(&panes);
        self.forget_running(&panes);
        let handles: Vec<SessionHandle> = {
            let mut sessions = lock(&self.sessions);
            panes.iter().filter_map(|p| sessions.remove(p)).collect()
        };
        for handle in handles {
            tokio::spawn(async move {
                let _ = handle.kill().await;
            });
        }
    }
}
