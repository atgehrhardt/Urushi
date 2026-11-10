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
//!                           │  ▲
//!                           ▼  │ retry
//!                     DeletionFailed
//! ```
//! A thread that fails while `Creating` is rolled back and never becomes visible, which
//! is why there is no `CreationFailed` state.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    BranchName, BranchPrefix, NamingError, ProjectId, RepoId, Slug, Status, TabId, ThreadId,
    Timestamp,
};

/// Where a thread is in its lifecycle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ThreadState {
    /// The worktree is being created.
    Creating,
    /// Usable.
    Ready,
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

    /// `Ready | DeletionFailed → Deleting`.
    pub fn begin_deletion(&mut self) -> Result<(), ThreadStateError> {
        self.transition(
            "delete",
            |s| matches!(s, ThreadState::Ready | ThreadState::DeletionFailed { .. }),
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
mod tests {
    use super::*;

    fn thread_spec() -> NewThread {
        NewThread {
            id: ThreadId::from_u128(0xabcd_1234),
            repo: RepoId::from_u128(1),
            project: None,
            title: "Refactor auth".into(),
            worktree_path: PathBuf::from("/data/worktrees/p/t"),
            base_ref: "main".into(),
            base_commit: "0123abcd".into(),
            created_at: Timestamp::from_unix_millis(0),
            branch: None,
        }
    }

    fn thread() -> Thread {
        Thread::new(thread_spec(), &BranchPrefix::default()).unwrap()
    }

    #[test]
    fn a_named_branch_is_used_as_is() {
        let spec = NewThread {
            branch: Some(BranchName::new("Primary").unwrap()),
            ..thread_spec()
        };
        let t = Thread::new(spec, &BranchPrefix::default()).unwrap();
        assert_eq!(t.primary_branch.as_str(), "Primary");
        assert_eq!(t.slug.as_str(), "refactor-auth");
    }

    #[test]
    fn threads_saved_before_the_repo_rename_still_load() {
        let mut json = serde_json::to_value(thread()).unwrap();
        let map = json.as_object_mut().unwrap();
        let repo = map.remove("repo").unwrap();
        map.insert("project".into(), repo);
        assert!(!map.contains_key("folder") && !map.contains_key("status"));
        let loaded: Thread = serde_json::from_value(json).unwrap();
        assert_eq!(loaded.repo, RepoId::from_u128(1));
        assert_eq!(loaded.project, None);
        assert!(!loaded.pinned);

        let mut filed = thread();
        filed.project = Some(ProjectId::from_u128(7));
        filed.status = Some(Status::new("Blocked").unwrap());
        let json = serde_json::to_string(&filed).unwrap();
        assert!(json.contains("\"folder\""), "{json}");
        assert_eq!(serde_json::from_str::<Thread>(&json).unwrap(), filed);
    }

    #[test]
    fn a_project_base_thread_works_on_the_given_branch_and_round_trips() {
        let main = BranchName::new("main").unwrap();
        let t = Thread::project_base(thread_spec(), main.clone(), true);
        assert_eq!(t.primary_branch, main);
        assert!(t.is_project_base() && t.is_attached());
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Thread>(&json).unwrap(), t);

        let plain = serde_json::to_string(&thread()).unwrap();
        assert!(
            !plain.contains("\"kind\""),
            "ordinary threads store no kind: {plain}"
        );
        let loaded: Thread = serde_json::from_str(&plain).unwrap();
        assert_eq!(loaded.kind, ThreadKind::Branch);
        assert!(!loaded.is_project_base() && !loaded.is_attached());
    }

    #[test]
    fn new_thread_names_its_branch() {
        let t = thread();
        assert_eq!(t.primary_branch.as_str(), "urushi/refactor-auth-abcd1234");
        assert_eq!(t.state, ThreadState::Creating);
    }

    #[test]
    fn lifecycle_happy_path_and_retry() {
        let mut t = thread();
        assert!(t.begin_deletion().is_err(), "cannot delete while creating");
        t.mark_ready().unwrap();
        assert!(t.is_ready());
        t.begin_deletion().unwrap();
        t.deletion_failed("offline").unwrap();
        assert!(matches!(t.state, ThreadState::DeletionFailed { .. }));
        t.begin_deletion().unwrap();
        assert_eq!(t.state, ThreadState::Deleting);
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let mut t = thread();
        assert!(t.deletion_failed("x").is_err());
        t.mark_ready().unwrap();
        let err = t.mark_ready().unwrap_err();
        assert_eq!(err.from, ThreadState::Ready);
    }
}
