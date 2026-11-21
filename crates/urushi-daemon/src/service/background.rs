//! Background tasks: the shells and monitors each pane's agent keeps open in the
//! background (ADR 0059).
//!
//! Claude's `Stop` hook reports how many background shells and monitors its input lists
//! (Urushi's `background` signal). Claude lists them only there. One that ends wakes
//! Claude for a turn, whose `Stop` reports again. They outlive the turn, so unlike
//! running, attention and interrupt keys leave them alone. A pane's entry goes when a
//! report says nothing is open, when its agent's session ends (`idle`, as the mux
//! reports it), and when the pane is gone. Like running, it lives only in memory,
//! doesn't change the workspace revision, and every change is broadcast as
//! `WorkspaceChanged`, with each tab listing its panes that have any open.

use std::collections::HashMap;

use urushi_core::{BackgroundTasks, Layout, PaneId};
use urushi_proto::v1;

use super::state::State;
use super::{Service, lock};

impl Service {
    /// Records what `pane`'s agent keeps open in the background. Broadcasts only on a
    /// change.
    pub(crate) fn set_background(&self, pane: PaneId, tasks: BackgroundTasks) {
        let changed = {
            let mut panes = lock(&self.background);
            if tasks.is_empty() {
                panes.remove(&pane).is_some()
            } else {
                panes.insert(pane, tasks) != Some(tasks)
            }
        };
        if changed {
            self.broadcast();
        }
    }

    /// Forgets panes that are gone. Their removal is broadcast by whoever removed them.
    pub(super) fn forget_background(&self, panes: &[PaneId]) {
        let mut background = lock(&self.background);
        for pane in panes {
            background.remove(pane);
        }
    }

    /// Adds each tab's (and the quick terminal's) panes with background tasks to
    /// `workspace`, which was made from `state` (same tab order).
    pub(super) fn add_background(&self, workspace: &mut v1::Workspace, state: &State) {
        let background = lock(&self.background);
        if background.is_empty() {
            return;
        }
        for (wire, tab) in workspace.tabs.iter_mut().zip(&state.tabs) {
            wire.background = wire_background(tab.layout(), &background);
        }
        if let (Some(wire), Some(quick)) = (&mut workspace.quick_terminal, &state.quick_terminal) {
            wire.background = wire_background(quick.layout(), &background);
        }
    }
}

/// The panes of `layout` with background tasks, for the wire.
fn wire_background(
    layout: &Layout,
    background: &HashMap<PaneId, BackgroundTasks>,
) -> Vec<v1::PaneBackground> {
    layout
        .panes()
        .into_iter()
        .filter_map(|pane| {
            let tasks = background.get(&pane)?;
            Some(v1::PaneBackground {
                pane_id: pane.to_string(),
                shells: tasks.shells,
                monitors: tasks.monitors,
            })
        })
        .collect()
}
