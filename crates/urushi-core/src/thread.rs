//! A thread: one unit of agent work, always backed by a git worktree.
//!
//! Most threads are [`ThreadKind::Branch`]: a new branch in a new worktree of their own.
//! A project's parent-branch thread ([`ThreadKind::ProjectBase`], ADR 0029) instead works
//! on an existing branch, possibly in a checkout the user already had. It never owns a
//! branch.
//!
//! # Lifecycle
//! ```text
//! Creating ──► Ready ──► Deleting ──► (removed)
//!              │  ▲        ▲  │  ▲
//!        shelve│  │unshelve│  ▼  │ retry
//!              ▼  │        │ DeletionFailed
//!             Shelved ─────┘
//! ```
//! A thread that fails while `Creating` is rolled back and never becomes visible, which
//! is why there is no `CreationFailed` state. A `Shelved` thread keeps its worktree,
//! tabs and layout, but none of its panes runs (ADR 0060).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod notes;

pub use notes::{MAX_THREAD_NOTES, NotesTooLong, thread_notes};

use crate::{
    BranchName, BranchPrefix, LabelColor, NamingError, ProjectId, RepoId, Slug, Status, TabId,
    ThreadId, Timestamp,
};

/// Where a thread is in its lifecycle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ThreadState {
    /// The worktree is being created.
    Creating,
    /// Usable.
    Ready,
    /// Put aside by the user to free resources: its worktree, tabs and layout are kept,
    /// but none of its panes runs until it is unshelved (ADR 0060).
    Shelved,
    /// Deletion is running.
    Deleting,
    /// Deletion stopped partway (e.g. offline while deleting a remote branch). Can be retried.
    DeletionFailed {
        /// Explanation to show the user.
        reason: String,
    },
}

/// An invalid lifecycle transition was attempted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("cannot {action} a thread that is {from:?}")]
pub struct ThreadStateError {
    /// The state the thread was in.
    pub from: ThreadState,
    /// What was attempted.
    pub action: &'static str,
}

/// What kind of thread it is, which decides what deleting it may destroy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThreadKind {
    /// A new branch in a new worktree, both owned by the thread.
    #[default]
    Branch,
    /// Its project's parent-branch thread, working on that existing branch. It owns no
    /// branch.
    ProjectBase {
        /// It runs in a checkout Urushi didn't create (e.g. the repo's own folder), so
        /// closing it leaves everything on disk alone.
        attached: bool,
    },
}

impl ThreadKind {
    /// Whether it is an ordinary thread (used to keep it out of stored JSON).
    pub fn is_branch(&self) -> bool {
        matches!(self, Self::Branch)
    }
}

/// What the daemon passes in to create a [`Thread`]. Ids, paths and time are supplied by
/// the caller, so this crate stays free of IO.
#[derive(Clone, Debug)]
pub struct NewThread {
    /// Id for the new thread.
    pub id: ThreadId,
    /// The repo it is a worktree of.
    pub repo: RepoId,
    /// The project to file it under, if any.
    pub project: Option<ProjectId>,
    /// Title entered by the user.
    pub title: String,
    /// Where the worktree will be created.
    pub worktree_path: PathBuf,
    /// The ref the user chose to branch from (e.g. `main`).
    pub base_ref: String,
    /// The commit `base_ref` resolved to at creation time.
    pub base_commit: String,
    /// Creation time.
    pub created_at: Timestamp,
    /// The branch to create, if the user named one. `None` generates
    /// `<prefix><slug>-<short id>`.
    pub branch: Option<BranchName>,
}

/// A thread. It owns one worktree and the tabs shown for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    /// Identity.
    pub id: ThreadId,
    /// The repo it is a worktree of. (Stored as `project` before repos were renamed.)
    #[serde(alias = "project")]
    pub repo: RepoId,
    /// The project it is filed under. `None`: ungrouped. Stored as `folder`, since the
    /// old name `project` now belongs to `repo`.
    #[serde(rename = "folder", default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectId>,
    /// Pinned threads are listed first within their group.
    #[serde(default)]
    pub pinned: bool,
    /// A user-chosen status. Threads with a status are grouped under it in the sidebar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// Display title.
    pub title: String,
    /// Slug made from the title when the thread was created. Used in the branch name.
    pub slug: Slug,
    /// The worktree directory, on the daemon's machine.
    pub worktree_path: PathBuf,
    /// The branch created for this thread.
    pub primary_branch: BranchName,
    /// What the user branched from.
    pub base_ref: String,
    /// The commit `base_ref` pointed to at creation.
    pub base_commit: String,
    /// Creation time.
    pub created_at: Timestamp,
    /// Lifecycle state.
    pub state: ThreadState,
    /// Tabs, in display order.
    pub tabs: Vec<TabId>,
    /// Ordinary or a project's parent-branch thread. Threads saved before kinds existed
    /// are ordinary.
    #[serde(default, skip_serializing_if = "ThreadKind::is_branch")]
    pub kind: ThreadKind,
    /// Marked unread by the user, so it stands out in the sidebar until it is opened
    /// again (ADR 0048).
    #[serde(default, skip_serializing_if = "is_false")]
    pub unread: bool,
    /// The user's own notes on the thread, in Markdown (see [`thread_notes`] and
    /// [`crate::markdown`]). Empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// The color the notes are shown in (ADR 0058). `None`: the clients' default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_color: Option<LabelColor>,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if takes a reference"
)]
fn is_false(b: &bool) -> bool {
    !*b
}

impl Thread {
    /// A new thread in the `Creating` state. Its primary branch is the one the user named,
    /// or `<prefix><slug>-<short id>`.
    pub fn new(spec: NewThread, prefix: &BranchPrefix) -> Result<Self, NamingError> {
        let slug = Slug::from_title(&spec.title);
        let primary_branch = match spec.branch.clone() {
            Some(branch) => branch,
            None => BranchName::for_thread(prefix, &slug, spec.id)?,
        };
        Ok(Self::build(spec, slug, primary_branch, ThreadKind::Branch))
    }

    /// A project's parent-branch thread, in the `Creating` state, working on `branch`
    /// (which already exists). `attached`: see [`ThreadKind::ProjectBase`].
    pub fn project_base(spec: NewThread, branch: BranchName, attached: bool) -> Self {
        let slug = Slug::from_title(&spec.title);
        Self::build(spec, slug, branch, ThreadKind::ProjectBase { attached })
    }

    fn build(spec: NewThread, slug: Slug, primary_branch: BranchName, kind: ThreadKind) -> Self {
        Self {
            id: spec.id,
            repo: spec.repo,
            project: spec.project,
            pinned: false,
            status: None,
            title: spec.title,
            slug,
            worktree_path: spec.worktree_path,
            primary_branch,
            base_ref: spec.base_ref,
            base_commit: spec.base_commit,
            created_at: spec.created_at,
            state: ThreadState::Creating,
            tabs: Vec::new(),
            kind,
            unread: false,
            notes: String::new(),
            notes_color: None,
        }
    }

    /// Whether it is its project's parent-branch thread.
    pub fn is_project_base(&self) -> bool {
        matches!(self.kind, ThreadKind::ProjectBase { .. })
    }

    /// Whether it runs in a checkout Urushi didn't create, which it must never remove.
    pub fn is_attached(&self) -> bool {
        matches!(self.kind, ThreadKind::ProjectBase { attached: true })
    }

    /// `Creating → Ready`, once the worktree exists.
    pub fn mark_ready(&mut self) -> Result<(), ThreadStateError> {
        self.transition(
            "mark ready",
            |s| matches!(s, ThreadState::Creating),
            ThreadState::Ready,
        )
    }

    /// `Ready → Shelved`: its panes are about to stop, and stay stopped until it is
    /// unshelved.
    pub fn shelve(&mut self) -> Result<(), ThreadStateError> {
        self.transition(
            "shelve",
            |s| matches!(s, ThreadState::Ready),
            ThreadState::Shelved,
        )
    }

    /// `Shelved → Ready`: its panes start again from their restore points.
    pub fn unshelve(&mut self) -> Result<(), ThreadStateError> {
        self.transition(
            "unshelve",
            |s| matches!(s, ThreadState::Shelved),
            ThreadState::Ready,
        )
    }

    /// Whether it is shelved: kept, but with no pane running.
    pub fn is_shelved(&self) -> bool {
        self.state == ThreadState::Shelved
    }

    /// `Ready | Shelved | DeletionFailed → Deleting`.
    pub fn begin_deletion(&mut self) -> Result<(), ThreadStateError> {
        self.transition(
            "delete",
            |s| {
                matches!(
                    s,
                    ThreadState::Ready | ThreadState::Shelved | ThreadState::DeletionFailed { .. }
                )
            },
            ThreadState::Deleting,
        )
    }

    /// `Deleting → DeletionFailed`.
    pub fn deletion_failed(&mut self, reason: impl Into<String>) -> Result<(), ThreadStateError> {
        self.transition(
            "fail deletion of",
            |s| matches!(s, ThreadState::Deleting),
            ThreadState::DeletionFailed {
                reason: reason.into(),
            },
        )
    }

    /// Whether the user can open terminals in it.
    pub fn is_ready(&self) -> bool {
        self.state == ThreadState::Ready
    }

    fn transition(
        &mut self,
        action: &'static str,
        allowed: impl FnOnce(&ThreadState) -> bool,
        next: ThreadState,
    ) -> Result<(), ThreadStateError> {
        if allowed(&self.state) {
            self.state = next;
            Ok(())
        } else {
            Err(ThreadStateError {
                from: self.state.clone(),
                action,
            })
        }
    }
}

#[cfg(test)]
mod tests;
