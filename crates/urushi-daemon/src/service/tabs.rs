//! Tabs, panes and layout operations.

use urushi_core::{Axis, Direction, PaneId, Tab, TabId, ThreadId};
use urushi_mux::TermSize;

use super::threads::DEFAULT_TAB_TITLE;
use super::{Service, ServiceError, ServiceResult, new_uuid};

impl Service {
    /// Opens a new tab with one shell in the thread's worktree.
    pub fn create_tab(&self, thread: ThreadId, title: &str) -> ServiceResult<Tab> {
        let (record, shell) = self.read(|s| {
            let record = s.thread(thread)?.thread.clone();
            if !record.is_ready() {
                return Err(ServiceError::Precondition("thread is not ready".into()));
            }
            Ok((
                record.clone(),
                s.effective_settings(record.project).default_shell,
            ))
        })?;
        let tab_id = TabId::from_uuid(new_uuid());
        let pane = PaneId::from_uuid(new_uuid());
        self.spawn_session(pane, &record, shell, TermSize::DEFAULT)?;
        let title = if title.trim().is_empty() {
            DEFAULT_TAB_TITLE
        } else {
            title.trim()
        };
        let tab = Tab::new(tab_id, thread, title, pane);
        let result = self.mutate(|state| {
            state.thread_mut(thread)?.thread.tabs.push(tab_id);
            state.tabs.push(tab.clone());
            Ok(tab)
        });
        if result.is_err() {
            self.kill_sessions([pane]);
        }
        result
    }

    /// Closes a tab and kills its terminals.
    pub fn close_tab(&self, tab: TabId) -> ServiceResult<()> {
        let panes = self.mutate(|state| {
            state.tab(tab)?;
            Ok(state.remove_tab(tab))
        })?;
        self.kill_sessions(panes);
        Ok(())
    }

    /// Renames a tab.
    pub fn rename_tab(&self, tab: TabId, title: &str) -> ServiceResult<()> {
        self.mutate(|state| {
            state.tab_mut(tab)?.set_title(title.trim());
            Ok(())
        })
    }

    /// Splits `pane`, starting a new shell on `side`. Returns the new pane.
    pub fn split_pane(&self, tab: TabId, pane: PaneId, side: Direction) -> ServiceResult<PaneId> {
        let (thread, shell) = self.read(|state| {
            let existing = state.tab(tab)?;
            if !existing.layout().contains(pane) {
                return Err(ServiceError::NotFound(format!("pane {pane} in tab {tab}")));
            }
            let thread = state.thread(existing.thread())?.thread.clone();
            let shell = state.effective_settings(thread.project).default_shell;
            Ok((thread, shell))
        })?;
        let new = PaneId::from_uuid(new_uuid());
        self.spawn_session(new, &thread, shell, TermSize::DEFAULT)?;
        let result = self.mutate(|state| {
            state
                .tab_mut(tab)?
                .split(pane, new, side)
                .map_err(|e| ServiceError::Invalid(e.to_string()))
        });
        match result {
            Ok(()) => Ok(new),
            Err(e) => {
                self.kill_sessions([new]);
                Err(e)
            }
        }
    }

    /// Closes a pane and kills its terminal. Closing a tab's last pane closes the tab.
    pub fn close_pane(&self, tab: TabId, pane: PaneId) -> ServiceResult<()> {
        self.mutate(|state| {
            if !state.tab(tab)?.layout().contains(pane) {
                return Err(ServiceError::NotFound(format!("pane {pane} in tab {tab}")));
            }
            state.remove_pane(pane);
            Ok(())
        })?;
        self.kill_sessions([pane]);
        Ok(())
    }

    /// Focuses a pane.
    pub fn focus_pane(&self, tab: TabId, pane: PaneId) -> ServiceResult<()> {
        self.layout_op(tab, |t| t.focus(pane))
    }

    /// Moves a divider (mouse drag). See `Layout::move_divider`.
    pub fn move_divider(
        &self,
        tab: TabId,
        anchor: PaneId,
        axis: Axis,
        position: f64,
    ) -> ServiceResult<()> {
        self.layout_op(tab, |t| t.move_divider(anchor, axis, position))
    }

    /// Moves a pane's edge (keyboard resize). See `Layout::resize`.
    pub fn resize_pane_edge(
        &self,
        tab: TabId,
        pane: PaneId,
        side: Direction,
        amount: f64,
    ) -> ServiceResult<()> {
        self.layout_op(tab, |t| t.resize(pane, side, amount))
    }

    /// Makes all panes in a tab equal in size.
    pub fn equalize(&self, tab: TabId) -> ServiceResult<()> {
        self.layout_op(tab, |t| {
            t.equalize();
            Ok(())
        })
    }

    fn layout_op(
        &self,
        tab: TabId,
        op: impl FnOnce(&mut Tab) -> Result<(), urushi_core::LayoutError>,
    ) -> ServiceResult<()> {
        self.mutate(|state| {
            op(state.tab_mut(tab)?).map_err(|e| ServiceError::Invalid(e.to_string()))
        })
    }
}
