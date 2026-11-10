//! Tabs, panes and layout operations.

use urushi_core::{Agent, Axis, Direction, PaneGroup, PaneId, Tab, TabId, Thread, ThreadId};
use urushi_mux::TermSize;

use super::threads::DEFAULT_TAB_TITLE;
use super::{PaneHome, Service, ServiceError, ServiceResult, State, new_uuid};

impl Service {
    /// Opens a new tab with one shell in the thread's worktree. With an `agent`, the
    /// agent's command is typed into that shell, and the tab is named after it.
    pub async fn create_tab(
        &self,
        thread: ThreadId,
        title: &str,
        agent: Option<Agent>,
    ) -> ServiceResult<Tab> {
        let (record, shell) = self.read(|s| {
            let record = s.thread(thread)?.thread.clone();
            if !record.is_ready() {
                return Err(ServiceError::Precondition("thread is not ready".into()));
            }
            Ok((
                record.clone(),
                s.effective_settings(record.repo).default_shell,
            ))
        })?;
        // Resolve the agent first, so a missing one leaves nothing behind.
        let command = match agent {
            Some(agent) => Some(self.agent_command_line(agent, shell.clone()).await?),
            None => None,
        };
        let tab_id = TabId::from_uuid(new_uuid());
        let pane = PaneId::from_uuid(new_uuid());
        self.spawn_session(pane, &record, shell, TermSize::DEFAULT)?;
        if let Some(command) = &command
            && let Err(e) = self.type_command(pane, command).await
        {
            self.kill_sessions([pane]);
            return Err(e);
        }
        let title = match (title.trim(), agent) {
            ("", Some(agent)) => agent.display_name(),
            ("", None) => DEFAULT_TAB_TITLE,
            (title, _) => title,
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

    /// Closes a tab (or the quick terminal) and kills its terminals.
    pub fn close_tab(&self, tab: TabId) -> ServiceResult<()> {
        let panes = self.mutate(|state| {
            state.group(tab)?;
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

    /// Splits `pane` in a tab (or the quick terminal), starting a new shell on `side`.
    /// Returns the new pane.
    pub fn split_pane(&self, tab: TabId, pane: PaneId, side: Direction) -> ServiceResult<PaneId> {
        let (thread, shell) = self.read(|state| {
            if !state.group(tab)?.layout().contains(pane) {
                return Err(ServiceError::NotFound(format!("pane {pane} in tab {tab}")));
            }
            pane_home(state, tab)
        })?;
        let home = thread.as_ref().map_or(PaneHome::Quick, PaneHome::Thread);
        let new = PaneId::from_uuid(new_uuid());
        self.spawn_session(new, home, shell, TermSize::DEFAULT)?;
        let result = self.mutate(|state| {
            state
                .group_mut(tab)?
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

    /// Closes a pane and kills its terminal. Closing the last pane of a tab (or of the
    /// quick terminal) closes that too.
    pub fn close_pane(&self, tab: TabId, pane: PaneId) -> ServiceResult<()> {
        self.mutate(|state| {
            if !state.group(tab)?.layout().contains(pane) {
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

    /// Applies a layout operation to a tab or the quick terminal.
    fn layout_op(
        &self,
        tab: TabId,
        op: impl FnOnce(&mut PaneGroup) -> Result<(), urushi_core::LayoutError>,
    ) -> ServiceResult<()> {
        self.mutate(|state| {
            op(state.group_mut(tab)?).map_err(|e| ServiceError::Invalid(e.to_string()))
        })
    }
}

/// Where new panes of `tab` start, and with which shell: a tab's thread and its repo's
/// shell, or (`None`) the quick terminal and the global shell.
fn pane_home(state: &State, tab: TabId) -> ServiceResult<(Option<Thread>, Option<String>)> {
    if state.quick(tab).is_some() {
        return Ok((None, state.settings.default_shell.clone()));
    }
    let thread = state.thread(state.tab(tab)?.thread())?.thread.clone();
    let shell = state.effective_settings(thread.repo).default_shell;
    Ok((Some(thread), shell))
}
