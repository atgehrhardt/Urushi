//! Shelving a thread to free what its terminals use, and unshelving it (ADR 0059).
//!
//! Shelving brings each of the thread's panes' restore points up to date
//! ([`crate::restore`]) while their processes still run, so an agent's conversation is
//! known, then marks the thread shelved and stops the processes. Its worktree, tabs and
//! layout stay. Unshelving starts every pane again from its restore point, as after a
//! daemon restart (ADR 0013): the shell in its directory below its old output, then the
//! agent resuming its conversation or the job running again. A shelved thread stays
//! shelved across daemon restarts, since only ready threads are restored at startup.

use std::collections::HashSet;

use urushi_core::{PaneId, ThreadId};

use super::restore::RestoreWork;
use super::{Service, ServiceError, ServiceResult};

/// Shown between an unshelved pane's old output and its new shell.
const UNSHELVED: &str = "\x1b[0;2m── unshelved ──\x1b[0m\r\n";

impl Service {
    /// Shelves `thread`: saves its panes' restore points, then stops their processes,
    /// keeping its worktree, tabs and layout. Only a ready thread can be shelved.
    pub async fn shelve_thread(&self, thread: ThreadId) -> ServiceResult<()> {
        let panes: HashSet<PaneId> = self.read(|s| {
            let record = s.thread(thread)?;
            if !record.thread.is_ready() {
                return Err(not_now(&record.thread.state, "shelve"));
            }
            Ok(s.thread_panes(thread).into_iter().collect())
        })?;
        // While they still run: a stopped agent no longer says which conversation it had.
        self.save_restore_points_of(Some(&panes)).await;
        // Shelved before the processes end, so nothing can start in it meanwhile.
        let panes = self.mutate(|state| {
            state
                .thread_mut(thread)?
                .thread
                .shelve()
                .map_err(|e| ServiceError::Precondition(e.to_string()))?;
            // Panes opened since the save have no restore point yet; they come back as
            // shells in the worktree.
            Ok(state.thread_panes(thread))
        })?;
        self.stop_sessions_keeping_panes(panes);
        Ok(())
    }

    /// Unshelves `thread`: starts each of its panes again from its restore point. Only a
    /// shelved thread can be unshelved.
    pub fn unshelve_thread(&self, thread: ThreadId) -> ServiceResult<()> {
        let work: Vec<RestoreWork> = self.mutate(|state| {
            state
                .thread_mut(thread)?
                .thread
                .unshelve()
                .map_err(|e| ServiceError::Precondition(e.to_string()))?;
            let record = state.thread(thread)?.thread.clone();
            let shell = state.effective_settings(record.repo).default_shell;
            Ok(state
                .thread_panes(thread)
                .into_iter()
                .map(|p| (p, Some(record.clone()), shell.clone()))
                .collect())
        })?;
        self.restore_panes(work, UNSHELVED);
        Ok(())
    }
}

/// The refusal for an action on a thread in a state that doesn't allow it.
fn not_now(state: &urushi_core::ThreadState, action: &str) -> ServiceError {
    let what = match state {
        urushi_core::ThreadState::Creating => "still being created",
        urushi_core::ThreadState::Shelved => "already shelved",
        urushi_core::ThreadState::Deleting | urushi_core::ThreadState::DeletionFailed { .. } => {
            "being deleted"
        }
        urushi_core::ThreadState::Ready => "ready",
    };
    ServiceError::Precondition(format!("can't {action} a thread that is {what}"))
}
