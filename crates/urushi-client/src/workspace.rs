//! The workspace as last reported by the daemon, with convenient lookups.

use urushi_core::Tab;
use urushi_proto::{convert, v1};

/// A read-only mirror of the daemon's workspace. Replace it wholesale on every
/// `WorkspaceChanged` event.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceMirror {
    workspace: v1::Workspace,
}

impl WorkspaceMirror {
    /// Wraps a workspace snapshot.
    pub fn new(workspace: v1::Workspace) -> Self {
        Self { workspace }
    }

    /// The raw snapshot.
    pub fn raw(&self) -> &v1::Workspace {
        &self.workspace
    }

    /// The snapshot's revision.
    pub fn revision(&self) -> u64 {
        self.workspace.revision
    }

    /// Repos, in sidebar order. (`v1::Project` is a repo on the wire, ADR 0010.)
    pub fn repos(&self) -> &[v1::Project] {
        &self.workspace.projects
    }

    /// A repo by id.
    pub fn repo(&self, id: &str) -> Option<&v1::Project> {
        self.workspace.projects.iter().find(|p| p.id == id)
    }

    /// A repo's threads, oldest first.
    pub fn threads_of_repo(&self, repo_id: &str) -> impl Iterator<Item = &v1::Thread> {
        self.workspace
            .threads
            .iter()
            .filter(move |t| t.project_id == repo_id)
    }

    /// Projects (user-made groups of threads; `v1::Folder` on the wire), in order.
    pub fn projects(&self) -> &[v1::Folder] {
        &self.workspace.folders
    }

    /// A project by id.
    pub fn project(&self, id: &str) -> Option<&v1::Folder> {
        self.workspace.folders.iter().find(|f| f.id == id)
    }

    /// Threads filed under `project_id` (`None`: ungrouped threads), oldest first.
    pub fn threads_in(&self, project_id: Option<&str>) -> impl Iterator<Item = &v1::Thread> {
        self.workspace
            .threads
            .iter()
            .filter(move |t| match project_id {
                Some(id) => t.folder_id == id,
                // Threads filed under a project this snapshot doesn't have count as ungrouped.
                None => t.folder_id.is_empty() || self.project(&t.folder_id).is_none(),
            })
    }

    /// Every status in use, sorted case-insensitively. Statuses are user-defined: a
    /// status exists while some thread has it.
    pub fn statuses(&self) -> Vec<String> {
        let mut statuses: Vec<String> = self
            .workspace
            .threads
            .iter()
            .filter(|t| !t.status.is_empty())
            .map(|t| t.status.clone())
            .collect();
        statuses.sort_by_key(|s| s.to_lowercase());
        statuses.dedup();
        statuses
    }

    /// A thread by id.
    pub fn thread(&self, id: &str) -> Option<&v1::Thread> {
        self.workspace.threads.iter().find(|t| t.id == id)
    }

    /// A thread's tabs, in display order.
    pub fn tabs_of(&self, thread_id: &str) -> Vec<&v1::Tab> {
        let Some(thread) = self.thread(thread_id) else {
            return Vec::new();
        };
        thread
            .tab_ids
            .iter()
            .filter_map(|id| self.workspace.tabs.iter().find(|t| &t.id == id))
            .collect()
    }

    /// Task boards, in tab order (protocol 1.5; empty from older daemons).
    pub fn boards(&self) -> &[v1::Board] {
        &self.workspace.boards
    }

    /// A board by id.
    pub fn board(&self, id: &str) -> Option<&v1::Board> {
        self.workspace.boards.iter().find(|b| b.id == id)
    }

    /// A column, with the board holding it.
    pub fn column(&self, id: &str) -> Option<(&v1::Board, &v1::BoardColumn)> {
        self.workspace
            .boards
            .iter()
            .find_map(|b| b.columns.iter().find(|c| c.id == id).map(|c| (b, c)))
    }

    /// A task, with the board and the column holding it.
    pub fn task(&self, id: &str) -> Option<(&v1::Board, &v1::BoardColumn, &v1::Task)> {
        self.workspace.boards.iter().find_map(|b| {
            b.columns
                .iter()
                .find_map(|c| c.tasks.iter().find(|t| t.id == id).map(|t| (b, c, t)))
        })
    }

    /// A subtask, with the task it belongs to.
    pub fn subtask(&self, id: &str) -> Option<(&v1::Task, &v1::Subtask)> {
        self.workspace
            .boards
            .iter()
            .flat_map(|b| &b.columns)
            .flat_map(|c| &c.tasks)
            .find_map(|t| t.subtasks.iter().find(|s| s.id == id).map(|s| (t, s)))
    }

    /// A tab, converted to the domain type (for geometry and focus logic).
    pub fn tab(&self, id: &str) -> Option<Tab> {
        let raw = self.workspace.tabs.iter().find(|t| t.id == id)?;
        convert::tab_from_proto(raw).ok()
    }

    /// The tab containing `pane_id`.
    pub fn tab_of_pane(&self, pane_id: &str) -> Option<&v1::Tab> {
        let pane = convert::parse_id::<urushi_core::PaneId>(pane_id, "pane_id").ok()?;
        self.workspace.tabs.iter().find(|t| {
            t.layout
                .as_ref()
                .and_then(|l| convert::layout_from_proto(l).ok())
                .is_some_and(|l| l.contains(pane))
        })
    }

    /// How many terminals (panes) are open, and in how many threads: what restarting
    /// the daemon would close.
    pub fn open_terminals(&self) -> (usize, usize) {
        let panes = |t: &v1::Tab| {
            t.layout
                .as_ref()
                .and_then(|l| convert::layout_from_proto(l).ok())
                .map_or(0, |l| l.panes().len())
        };
        let mut threads: Vec<&str> = Vec::new();
        let mut total = 0;
        for tab in &self.workspace.tabs {
            let n = panes(tab);
            if n > 0 {
                total += n;
                if !threads.contains(&tab.thread_id.as_str()) {
                    threads.push(&tab.thread_id);
                }
            }
        }
        (total, threads.len())
    }

    /// Finds an entity id by unique prefix among repos, projects, threads, tabs, panes,
    /// boards, columns and tasks, e.g. so the CLI accepts short ids. `None` if nothing or more than one thing matches.
    pub fn resolve(&self, prefix: &str) -> Option<String> {
        let panes = self
            .workspace
            .tabs
            .iter()
            .filter_map(|t| t.layout.as_ref())
            .flat_map(|l| {
                convert::layout_from_proto(l)
                    .map(|l| {
                        l.panes()
                            .into_iter()
                            .map(|p| p.to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            });
        let ids: Vec<String> = self
            .workspace
            .projects
            .iter()
            .map(|p| p.id.clone())
            .chain(self.workspace.folders.iter().map(|f| f.id.clone()))
            .chain(self.workspace.threads.iter().map(|t| t.id.clone()))
            .chain(self.workspace.tabs.iter().map(|t| t.id.clone()))
            .chain(panes)
            .chain(self.workspace.boards.iter().flat_map(|b| {
                std::iter::once(b.id.clone()).chain(b.columns.iter().flat_map(|c| {
                    std::iter::once(c.id.clone()).chain(c.tasks.iter().flat_map(|t| {
                        std::iter::once(t.id.clone()).chain(t.subtasks.iter().map(|s| s.id.clone()))
                    }))
                }))
            }))
            .collect();
        if let Some(exact) = ids.iter().find(|id| *id == prefix) {
            return Some(exact.clone());
        }
        let matches: Vec<&String> = ids
            .iter()
            .filter(|id| id.starts_with(prefix) || id.ends_with(prefix))
            .collect();
        match matches.as_slice() {
            [one] => Some((*one).clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, folder: &str, status: &str) -> v1::Thread {
        v1::Thread {
            id: id.into(),
            folder_id: folder.into(),
            status: status.into(),
            ..Default::default()
        }
    }

    #[test]
    fn threads_are_grouped_by_project_and_statuses_come_from_threads() {
        let mirror = WorkspaceMirror::new(v1::Workspace {
            folders: vec![v1::Folder {
                id: "f".into(),
                ..Default::default()
            }],
            threads: vec![
                thread("a", "f", "review"),
                thread("b", "", "Blocked"),
                thread("c", "gone", "review"),
                thread("d", "f", ""),
            ],
            ..Default::default()
        });
        let ids = |p: Option<&str>| {
            mirror
                .threads_in(p)
                .map(|t| t.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(Some("f")), ["a", "d"]);
        assert_eq!(ids(None), ["b", "c"], "unknown projects count as ungrouped");
        assert_eq!(mirror.statuses(), ["Blocked", "review"]);
    }
}
