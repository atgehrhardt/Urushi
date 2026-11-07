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

    /// Projects, in sidebar order.
    pub fn projects(&self) -> &[v1::Project] {
        &self.workspace.projects
    }

    /// A project by id.
    pub fn project(&self, id: &str) -> Option<&v1::Project> {
        self.workspace.projects.iter().find(|p| p.id == id)
    }

    /// A project's threads, oldest first.
    pub fn threads_of(&self, project_id: &str) -> impl Iterator<Item = &v1::Thread> {
        self.workspace
            .threads
            .iter()
            .filter(move |t| t.project_id == project_id)
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

    /// Finds an entity id by unique prefix among projects, threads, tabs and panes, e.g.
    /// so the CLI accepts short ids. `None` if nothing or more than one thing matches.
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
            .chain(self.workspace.threads.iter().map(|t| t.id.clone()))
            .chain(self.workspace.tabs.iter().map(|t| t.id.clone()))
            .chain(panes)
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
