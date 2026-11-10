//! The in-memory workspace state and its pure helpers (lookups, invariants, wire snapshot).

use urushi_core::{
    Board, EffectiveSettings, PaneId, Project, ProjectId, Repo, RepoId, Settings, Tab, TabId,
    ThreadId, ThreadState,
};
use urushi_proto::{convert, v1};

use super::{ServiceError, ServiceResult};
use crate::store::{Document, FORMAT_VERSION, ThreadRecord};

/// Everything the daemon knows about the workspace.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) settings: Settings,
    pub(crate) repos: Vec<Repo>,
    pub(crate) projects: Vec<Project>,
    pub(crate) threads: Vec<ThreadRecord>,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) boards: Vec<Board>,
    pub(crate) revision: u64,
}

impl State {
    pub(crate) fn from_document(doc: Document) -> Self {
        Self {
            settings: doc.settings,
            repos: doc.repos,
            projects: doc.projects,
            threads: doc.threads,
            tabs: doc.tabs,
            boards: doc.boards,
            revision: 0,
        }
    }

    pub(crate) fn to_document(&self) -> Document {
        Document {
            format: FORMAT_VERSION,
            settings: self.settings.clone(),
            repos: self.repos.clone(),
            projects: self.projects.clone(),
            threads: self.threads.clone(),
            tabs: self.tabs.clone(),
            boards: self.boards.clone(),
        }
    }

    pub(crate) fn to_proto(&self) -> v1::Workspace {
        v1::Workspace {
            revision: self.revision,
            projects: self.repos.iter().map(convert::repo_to_proto).collect(),
            folders: self
                .projects
                .iter()
                .map(convert::project_to_proto)
                .collect(),
            threads: self
                .threads
                .iter()
                .map(|r| convert::thread_to_proto(&r.thread))
                .collect(),
            tabs: self.tabs.iter().map(convert::tab_to_proto).collect(),
            boards: self.boards.iter().map(convert::board_to_proto).collect(),
        }
    }

    /// After a crash, threads stuck mid-creation or mid-deletion are marked
    /// `DeletionFailed`, so the user can finish deleting them through the normal
    /// (warned) flow.
    pub(crate) fn repair_interrupted(&mut self) {
        for record in &mut self.threads {
            let reason = match record.thread.state {
                ThreadState::Creating => "creation was interrupted",
                ThreadState::Deleting => "deletion was interrupted",
                _ => continue,
            };
            record.thread.state = ThreadState::DeletionFailed {
                reason: reason.into(),
            };
        }
        // Drop tabs whose thread no longer exists.
        let threads: Vec<ThreadId> = self.threads.iter().map(|r| r.thread.id).collect();
        self.tabs.retain(|t| threads.contains(&t.thread()));
        // Ungroup threads whose project no longer exists.
        let projects: Vec<ProjectId> = self.projects.iter().map(|p| p.id).collect();
        for record in &mut self.threads {
            if record
                .thread
                .project
                .is_some_and(|p| !projects.contains(&p))
            {
                record.thread.project = None;
            }
        }
    }

    pub(crate) fn repo(&self, id: RepoId) -> ServiceResult<&Repo> {
        self.repos
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| ServiceError::NotFound(format!("repo {id}")))
    }

    pub(crate) fn repo_mut(&mut self, id: RepoId) -> ServiceResult<&mut Repo> {
        self.repos
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| ServiceError::NotFound(format!("repo {id}")))
    }

    pub(crate) fn project(&self, id: ProjectId) -> ServiceResult<&Project> {
        self.projects
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| ServiceError::NotFound(format!("project {id}")))
    }

    pub(crate) fn project_mut(&mut self, id: ProjectId) -> ServiceResult<&mut Project> {
        self.projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| ServiceError::NotFound(format!("project {id}")))
    }

    /// The project's parent-branch thread, if it has one (in any state).
    pub(crate) fn project_base_thread(&self, project: ProjectId) -> Option<&ThreadRecord> {
        self.threads
            .iter()
            .find(|r| r.thread.project == Some(project) && r.thread.is_project_base())
    }

    pub(crate) fn thread(&self, id: ThreadId) -> ServiceResult<&ThreadRecord> {
        self.threads
            .iter()
            .find(|r| r.thread.id == id)
            .ok_or_else(|| ServiceError::NotFound(format!("thread {id}")))
    }

    pub(crate) fn thread_mut(&mut self, id: ThreadId) -> ServiceResult<&mut ThreadRecord> {
        self.threads
            .iter_mut()
            .find(|r| r.thread.id == id)
            .ok_or_else(|| ServiceError::NotFound(format!("thread {id}")))
    }

    pub(crate) fn tab(&self, id: TabId) -> ServiceResult<&Tab> {
        self.tabs
            .iter()
            .find(|t| t.id() == id)
            .ok_or_else(|| ServiceError::NotFound(format!("tab {id}")))
    }

    pub(crate) fn tab_mut(&mut self, id: TabId) -> ServiceResult<&mut Tab> {
        self.tabs
            .iter_mut()
            .find(|t| t.id() == id)
            .ok_or_else(|| ServiceError::NotFound(format!("tab {id}")))
    }

    /// Settings for `repo`, with its overrides applied (globals if it doesn't exist).
    pub(crate) fn effective_settings(&self, repo: RepoId) -> EffectiveSettings {
        let overrides = self
            .repos
            .iter()
            .find(|p| p.id == repo)
            .map(|p| p.settings.clone())
            .unwrap_or_default();
        self.settings.for_repo(&overrides)
    }

    /// Every pane in the thread's tabs.
    pub(crate) fn thread_panes(&self, thread: ThreadId) -> Vec<PaneId> {
        self.tabs
            .iter()
            .filter(|t| t.thread() == thread)
            .flat_map(|t| t.layout().panes())
            .collect()
    }

    /// Removes a tab and its reference from its thread. Returns its panes.
    pub(crate) fn remove_tab(&mut self, tab: TabId) -> Vec<PaneId> {
        let Some(index) = self.tabs.iter().position(|t| t.id() == tab) else {
            return Vec::new();
        };
        let removed = self.tabs.remove(index);
        if let Some(record) = self
            .threads
            .iter_mut()
            .find(|r| r.thread.id == removed.thread())
        {
            record.thread.tabs.retain(|t| *t != tab);
        }
        removed.layout().panes()
    }

    /// Removes a pane wherever it is. If it was a tab's last pane, the tab is removed too.
    pub(crate) fn remove_pane(&mut self, pane: PaneId) {
        let Some(tab_id) = self
            .tabs
            .iter()
            .find(|t| t.layout().contains(pane))
            .map(Tab::id)
        else {
            return;
        };
        let last = self
            .tabs
            .iter()
            .find(|t| t.id() == tab_id)
            .is_some_and(|t| t.layout().len() == 1);
        if last {
            self.remove_tab(tab_id);
        } else if let Some(tab) = self.tabs.iter_mut().find(|t| t.id() == tab_id) {
            let _ = tab.close_pane(pane);
        }
    }
}
