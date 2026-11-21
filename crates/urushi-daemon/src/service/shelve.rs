//! Shelving a thread to free what its terminals use, and unshelving it (ADR 0060).
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
use super::{Service, ServiceError, ServiceResult, lock};

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
    ///
    /// The terminals start before the thread is marked ready: clients attach its panes as
    /// soon as the workspace shows it ready, and would find no terminal yet.
    pub fn unshelve_thread(&self, thread: ThreadId) -> ServiceResult<()> {
        let _one_at_a_time = lock(&self.unshelving);
        let work: Vec<RestoreWork> = self.read(|state| {
            let record = state.thread(thread)?.thread.clone();
            // Refused as marking it would be, before anything starts.
            record
                .clone()
                .unshelve()
                .map_err(|e| ServiceError::Precondition(e.to_string()))?;
            let shell = state.effective_settings(record.repo).default_shell;
            Ok(state
                .thread_panes(thread)
                .into_iter()
                .map(|p| (p, Some(record.clone()), shell.clone()))
                .collect())
        })?;
        let panes: Vec<PaneId> = work.iter().map(|(pane, ..)| *pane).collect();
        self.restore_panes(work, UNSHELVED);
        let marked = self.mutate(|state| {
            state
                .thread_mut(thread)?
                .thread
                .unshelve()
                .map_err(|e| ServiceError::Precondition(e.to_string()))
        });
        if marked.is_err() {
            // Changed meanwhile (being deleted): stop the terminals again, keeping the panes.
            self.stop_sessions_keeping_panes(panes);
        }
        marked
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
