//! Threads on their way out. Once the user confirms the deletion warning, the thread
//! leaves the GUI at once, and the daemon removes its worktree and branches in the
//! background. That can take seconds when remote branches are deleted.
//!
//! The app keeps the workspace exactly as the daemon reported it, and shows a copy
//! without the threads being deleted:
//! - threads the daemon reports as `Deleting`, from any client;
//! - threads this app asked to delete whose removal the daemon hasn't reported yet.
//!   They hide before the daemon even starts, and they stay hidden if the answer to
//!   `DeleteThread` arrives before the workspace that no longer lists them.
//!
//! If the daemon refuses, the thread shows again. A thread whose deletion failed
//! partway comes back as `DeletionFailed`, so the user can retry through the warning.

use std::collections::HashSet;

use urushi_client::{WorkspaceMirror, v1};

/// The daemon's last reported workspace, and the deletions this app is waiting on.
#[derive(Debug, Default)]
pub struct Deletions {
    /// The workspace as the daemon last reported it, deleting threads included.
    reported: v1::Workspace,
    /// Threads this app asked to delete that the daemon still lists.
    pending: HashSet<String>,
}

impl Deletions {
    /// Keeps the daemon's latest workspace. A pending deletion it no longer lists is done.
    pub fn report(&mut self, workspace: v1::Workspace) {
        self.pending
            .retain(|id| workspace.threads.iter().any(|t| &t.id == id));
        self.reported = workspace;
    }

    /// The user confirmed deleting `thread`: hide it from now on.
    pub fn start(&mut self, thread: String) {
        self.pending.insert(thread);
    }

    /// The daemon refused to delete `thread`, or couldn't be asked: show it again
    /// (unless the daemon reports it as `Deleting` after all).
    pub fn failed(&mut self, thread: &str) {
        self.pending.remove(thread);
    }

    /// Whether this app is waiting for `thread` to be deleted.
    #[must_use]
    pub fn is_pending(&self, thread: &str) -> bool {
        self.pending.contains(thread)
    }

    /// A thread as the daemon last reported it, hidden or not (e.g. for its title in
    /// an error message).
    #[must_use]
    pub fn reported_thread(&self, thread: &str) -> Option<&v1::Thread> {
        self.reported.threads.iter().find(|t| t.id == thread)
    }

    /// The workspace the GUI shows: the reported one without the threads being
    /// deleted, or their tabs.
    #[must_use]
    pub fn visible(&self) -> WorkspaceMirror {
        let mut workspace = self.reported.clone();
        let hidden: HashSet<String> = workspace
            .threads
            .iter()
            .filter(|t| self.hides(t))
            .map(|t| t.id.clone())
            .collect();
        if !hidden.is_empty() {
            workspace.threads.retain(|t| !hidden.contains(&t.id));
            workspace.tabs.retain(|t| !hidden.contains(&t.thread_id));
        }
        WorkspaceMirror::new(workspace)
    }

    fn hides(&self, thread: &v1::Thread) -> bool {
        thread.state == v1::ThreadState::Deleting as i32 || self.pending.contains(&thread.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, state: v1::ThreadState) -> v1::Thread {
        v1::Thread {
            id: id.into(),
            title: id.to_uppercase(),
            state: state.into(),
            ..Default::default()
        }
    }

    fn workspace(threads: Vec<v1::Thread>) -> v1::Workspace {
        let tabs = threads
            .iter()
            .map(|t| v1::Tab {
                id: format!("tab-{}", t.id),
                thread_id: t.id.clone(),
                ..Default::default()
            })
            .collect();
        v1::Workspace {
            threads,
            tabs,
            ..Default::default()
        }
    }

    fn shown(d: &Deletions) -> (Vec<String>, Vec<String>) {
        let visible = d.visible();
        (
            visible.raw().threads.iter().map(|t| t.id.clone()).collect(),
            visible.raw().tabs.iter().map(|t| t.id.clone()).collect(),
        )
    }

    #[test]
    fn hides_threads_the_daemon_is_deleting_but_not_failed_ones() {
        let mut d = Deletions::default();
        d.report(workspace(vec![
            thread("a", v1::ThreadState::Ready),
            thread("b", v1::ThreadState::Deleting),
            thread("c", v1::ThreadState::DeletionFailed),
        ]));
        assert_eq!(
            shown(&d),
            (
                vec!["a".to_owned(), "c".to_owned()],
                vec!["tab-a".to_owned(), "tab-c".to_owned()]
            )
        );
        assert!(d.reported_thread("b").is_some(), "still known, just hidden");
    }

    #[test]
    fn a_confirmed_deletion_hides_at_once_until_the_daemon_drops_the_thread() {
        let mut d = Deletions::default();
        d.report(workspace(vec![
            thread("a", v1::ThreadState::Ready),
            thread("b", v1::ThreadState::Ready),
        ]));
        d.start("b".into());
        assert_eq!(shown(&d).0, vec!["a".to_owned()]);

        // The daemon hasn't started yet: still hidden.
        d.report(workspace(vec![
            thread("a", v1::ThreadState::Ready),
            thread("b", v1::ThreadState::Ready),
        ]));
        assert!(d.is_pending("b"));
        assert_eq!(shown(&d).0, vec!["a".to_owned()]);

        // Gone: nothing left to wait for.
        d.report(workspace(vec![thread("a", v1::ThreadState::Ready)]));
        assert!(!d.is_pending("b"));
    }

    #[test]
    fn a_refused_deletion_shows_the_thread_again() {
        let mut d = Deletions::default();
        d.report(workspace(vec![thread("a", v1::ThreadState::Ready)]));
        d.start("a".into());
        assert!(shown(&d).0.is_empty());
        d.failed("a");
        assert_eq!(shown(&d).0, vec!["a".to_owned()]);
    }
}
