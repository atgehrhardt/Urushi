//! Threads on their way in. Creating a thread waits for the daemon to update the base
//! from its remote and make a worktree, which can take seconds. So the New Thread dialog
//! closes as soon as it is submitted, and a placeholder stands in for the thread in the
//! sidebar and the main area until the daemon answers (ADR 0055).
//!
//! The placeholder lives only in this app: the daemon still records a thread only once
//! its worktree exists (invariant 8). When the answer comes, the placeholder goes: the
//! new thread is shown if the placeholder still was, and on failure the dialog comes
//! back with what the user typed, so nothing is lost.

use crate::state::Dialog;

/// One thread being created.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingThread {
    /// This app's number for it (not a thread id: the daemon hasn't made one yet).
    pub key: u64,
    /// The repo it is made in.
    pub repo: String,
    /// The project it is filed under.
    pub project: Option<String>,
    /// Its title, as it will show.
    pub title: String,
    /// Whether it was started for a board task.
    pub for_task: bool,
    /// The dialog as submitted, to bring back if creating fails.
    pub dialog: Dialog,
}

/// The threads this app asked the daemon to create, oldest first.
#[derive(Debug, Default)]
pub struct Creations {
    last: u64,
    pending: Vec<PendingThread>,
}

impl Creations {
    /// Records a create about to be sent; returns its key.
    pub fn start(
        &mut self,
        repo: String,
        project: Option<String>,
        title: String,
        dialog: Dialog,
    ) -> u64 {
        self.last += 1;
        let for_task = matches!(&dialog, Dialog::NewThread { task: Some(_), .. });
        self.pending.push(PendingThread {
            key: self.last,
            repo,
            project,
            title,
            for_task,
            dialog,
        });
        self.last
    }

    /// The daemon answered for `key`: it is no longer pending.
    pub fn finish(&mut self, key: u64) -> Option<PendingThread> {
        let at = self.pending.iter().position(|p| p.key == key)?;
        Some(self.pending.remove(at))
    }

    /// The create with this key, while it is pending.
    #[must_use]
    pub fn get(&self, key: u64) -> Option<&PendingThread> {
        self.pending.iter().find(|p| p.key == key)
    }

    /// Creates filed under `project` (`None`: in no project).
    pub fn in_project<'a>(
        &'a self,
        project: Option<&'a str>,
    ) -> impl Iterator<Item = &'a PendingThread> + 'a {
        self.pending
            .iter()
            .filter(move |p| p.project.as_deref() == project)
    }

    /// Whether anything is being created.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> Dialog {
        Dialog::AddProject {
            path: urushi_ui::TextField::default(),
        }
    }

    #[test]
    fn pending_creates_are_kept_until_answered() {
        let mut c = Creations::default();
        let a = c.start("r".into(), None, "A".into(), dialog());
        let b = c.start("r".into(), Some("p".into()), "B".into(), dialog());
        assert_ne!(a, b);
        assert_eq!(c.in_project(None).count(), 1);
        assert_eq!(
            c.in_project(Some("p")).map(|p| p.key).collect::<Vec<_>>(),
            vec![b]
        );
        assert_eq!(c.finish(a).map(|p| p.title), Some("A".into()));
        assert!(c.finish(a).is_none());
        assert!(c.get(b).is_some());
        c.finish(b);
        assert!(c.is_empty());
    }
}
