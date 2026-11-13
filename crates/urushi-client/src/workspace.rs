//! The workspace as last reported by the daemon, with convenient lookups.

use urushi_core::{PaneGroup, PaneId, QuickTerminal, Tab};
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

    /// The raw snapshot, taken out.
    pub fn into_raw(self) -> v1::Workspace {
        self.workspace
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

    /// A project's parent-branch thread (ADR 0029), if it has one.
    pub fn project_base_thread(&self, project_id: &str) -> Option<&v1::Thread> {
        self.workspace
            .threads
            .iter()
            .find(|t| t.project_base && t.folder_id == project_id)
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

    /// A card (a task on the board, not a subtask), with the board and the column
    /// holding it. [`Self::find_task`] finds subtasks too.
    pub fn task(&self, id: &str) -> Option<(&v1::Board, &v1::BoardColumn, &v1::Task)> {
        self.workspace.boards.iter().find_map(|b| {
            b.columns
                .iter()
                .find_map(|c| c.tasks.iter().find(|t| t.id == id).map(|t| (b, c, t)))
        })
    }

    /// A tab, converted to the domain type (for geometry and focus logic).
    pub fn tab(&self, id: &str) -> Option<Tab> {
        let raw = self.workspace.tabs.iter().find(|t| t.id == id)?;
        convert::tab_from_proto(raw).ok()
    }

    /// The tab containing `pane_id`.
    pub fn tab_of_pane(&self, pane_id: &str) -> Option<&v1::Tab> {
        let pane = convert::parse_id::<PaneId>(pane_id, "pane_id").ok()?;
        self.workspace
            .tabs
            .iter()
            .find(|t| panes(t.layout.as_ref()).contains(&pane))
    }

    /// The daemon's quick terminal as sent (protocol 1.20; `None` when none is open).
    pub fn quick_terminal_raw(&self) -> Option<&v1::QuickTerminal> {
        self.workspace.quick_terminal.as_ref()
    }

    /// The quick terminal, converted to the domain type.
    pub fn quick_terminal(&self) -> Option<QuickTerminal> {
        convert::quick_terminal_from_proto(self.quick_terminal_raw()?).ok()
    }

    /// The panes of the tab or quick terminal with this id (layout requests name either
    /// by a tab id).
    pub fn group(&self, id: &str) -> Option<PaneGroup> {
        match self.quick_terminal().filter(|q| q.id().to_string() == id) {
            Some(quick) => Some(quick.group().clone()),
            None => self.tab(id).map(|t| t.group().clone()),
        }
    }

    /// The id of the tab or quick terminal holding `pane_id`.
    pub fn owner_of_pane(&self, pane_id: &str) -> Option<String> {
        let pane = convert::parse_id::<PaneId>(pane_id, "pane_id").ok()?;
        self.quick_terminal_raw()
            .filter(|q| panes(q.layout.as_ref()).contains(&pane))
            .map(|q| q.id.clone())
            .or_else(|| self.tab_of_pane(pane_id).map(|t| t.id.clone()))
    }

    /// How many terminals (panes) are open, and in how many threads: what restarting
    /// the daemon would close. The quick terminal's panes count, in no thread.
    pub fn open_terminals(&self) -> (usize, usize) {
        let mut threads: Vec<&str> = Vec::new();
        let mut total = 0;
        for tab in &self.workspace.tabs {
            let n = panes(tab.layout.as_ref()).len();
            if n > 0 {
                total += n;
                if !threads.contains(&tab.thread_id.as_str()) {
                    threads.push(&tab.thread_id);
                }
            }
        }
        let quick = self
            .quick_terminal_raw()
            .map_or(0, |q| panes(q.layout.as_ref()).len());
        (total + quick, threads.len())
    }

    /// How many panes have an agent working right now (ADR 0039): what restarting the
    /// daemon would interrupt mid-task.
    pub fn running_agents(&self) -> usize {
        let tabs: usize = self.workspace.tabs.iter().map(|t| t.running.len()).sum();
        tabs + self.quick_terminal_raw().map_or(0, |q| q.running.len())
    }

    /// Finds an entity id by unique prefix among repos, projects, threads, tabs, the
    /// quick terminal, panes, boards, columns, tasks (subtasks at any depth too) and
    /// attachments, e.g. so the CLI accepts short ids. `None` if nothing or more than one
    /// thing matches.
    pub fn resolve(&self, prefix: &str) -> Option<String> {
        let quick = self.quick_terminal_raw();
        let panes = self
            .workspace
            .tabs
            .iter()
            .map(|t| t.layout.as_ref())
            .chain(quick.map(|q| q.layout.as_ref()))
            .flat_map(panes)
            .map(|p| p.to_string());
        let ids: Vec<String> = self
            .workspace
            .projects
            .iter()
            .map(|p| p.id.clone())
            .chain(self.workspace.folders.iter().map(|f| f.id.clone()))
            .chain(self.workspace.threads.iter().map(|t| t.id.clone()))
            .chain(self.workspace.tabs.iter().map(|t| t.id.clone()))
            .chain(quick.map(|q| q.id.clone()))
            .chain(panes)
            .chain(self.workspace.boards.iter().flat_map(|b| {
                std::iter::once(b.id.clone()).chain(b.columns.iter().flat_map(|c| {
                    std::iter::once(c.id.clone()).chain(c.tasks.iter().flat_map(|t| {
                        crate::TaskRef::Card(t)
                            .subtree_ids()
                            .into_iter()
                            .map(str::to_owned)
                    }))
                }))
            }))
            .chain(
                self.all_tasks()
                    .flat_map(crate::TaskRef::attachments)
                    .map(|a| a.id.clone()),
            )
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

/// The panes of a wire layout (none if it is missing or broken).
fn panes(layout: Option<&v1::LayoutNode>) -> Vec<PaneId> {
    layout
        .and_then(|l| convert::layout_from_proto(l).ok())
        .map(|l| l.panes())
        .unwrap_or_default()
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
        assert!(mirror.project_base_thread("f").is_none());
    }

    #[test]
    fn a_project_finds_its_parent_branch_thread() {
        let base = v1::Thread {
            project_base: true,
            ..thread("m", "f", "")
        };
        let mirror = WorkspaceMirror::new(v1::Workspace {
            threads: vec![thread("a", "f", ""), base, thread("x", "g", "")],
            ..Default::default()
        });
        assert_eq!(mirror.project_base_thread("f").unwrap().id, "m");
        assert!(mirror.project_base_thread("g").is_none());
    }

    #[test]
    fn the_quick_terminal_is_found_by_its_id_and_its_panes() {
        let pane = PaneId::from_u128(7);
        let id = urushi_core::TabId::from_u128(3).to_string();
        let mirror = WorkspaceMirror::new(v1::Workspace {
            quick_terminal: Some(convert::quick_terminal_to_proto(&QuickTerminal::new(
                urushi_core::TabId::from_u128(3),
                pane,
            ))),
            ..Default::default()
        });
        assert_eq!(mirror.group(&id).unwrap().focused(), pane);
        assert!(mirror.group("nope").is_none());
        assert_eq!(mirror.owner_of_pane(&pane.to_string()), Some(id.clone()));
        assert_eq!(mirror.open_terminals(), (1, 0), "in no thread");
        assert_eq!(mirror.resolve(&id[id.len() - 4..]), Some(id));
    }

    #[test]
    fn running_agents_are_counted_in_tabs_and_the_quick_terminal() {
        let running = |n: u128| v1::PaneRunning {
            pane_id: PaneId::from_u128(n).to_string(),
            since_ms: 1,
        };
        let mut quick = convert::quick_terminal_to_proto(&QuickTerminal::new(
            urushi_core::TabId::from_u128(3),
            PaneId::from_u128(7),
        ));
        quick.running = vec![running(7)];
        let mirror = WorkspaceMirror::new(v1::Workspace {
            tabs: vec![
                v1::Tab {
                    running: vec![running(1), running(2)],
                    ..Default::default()
                },
                v1::Tab::default(),
            ],
            quick_terminal: Some(quick),
            ..Default::default()
        });
        assert_eq!(mirror.running_agents(), 3);
        assert_eq!(
            WorkspaceMirror::new(v1::Workspace::default()).running_agents(),
            0
        );
    }
}
