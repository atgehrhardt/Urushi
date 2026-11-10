//! Running agents as the app sees it (ADR 0033): which threads, tabs and task cards have
//! a pane whose agent is working, and the one [`Indicator`] each shows, combining that
//! with attention (ADR 0017). The daemon owns both; they arrive on each tab of the
//! workspace.

use urushi_client::v1;

use crate::attention::linked_threads;
use crate::state::AppState;

/// What a thread, tab, task card or subtask row shows about its agents, ordered by
/// priority: something waiting for the user outranks an agent at work, which outranks
/// one that finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Indicator {
    /// An agent finished its response.
    Done,
    /// An agent is working: it took a prompt, uses a tool or runs a subagent.
    Running,
    /// Something waits for the user.
    Waiting,
}

impl From<v1::AttentionKind> for Indicator {
    /// An unknown kind (from a newer daemon) counts as waiting, as in `attention::urgency`.
    fn from(kind: v1::AttentionKind) -> Self {
        match kind {
            v1::AttentionKind::Done => Self::Done,
            v1::AttentionKind::Waiting | v1::AttentionKind::Unspecified => Self::Waiting,
        }
    }
}

/// The indicator of a set of panes: their attention, and whether any is running.
fn indicator_of(attention: &[v1::PaneAttention], running: &[v1::PaneRunning]) -> Option<Indicator> {
    let attention = attention.iter().map(|a| Indicator::from(a.kind())).max();
    let running = (!running.is_empty()).then_some(Indicator::Running);
    attention.max(running)
}

impl AppState {
    /// What a thread shows: the most important state among its panes.
    pub fn thread_indicator(&self, thread: &str) -> Option<Indicator> {
        self.workspace
            .raw()
            .tabs
            .iter()
            .filter(|t| t.thread_id == thread)
            .filter_map(|t| indicator_of(&t.attention, &t.running))
            .max()
    }

    /// What a tab shows.
    pub fn tab_indicator(&self, tab: &str) -> Option<Indicator> {
        self.workspace
            .raw()
            .tabs
            .iter()
            .find(|t| t.id == tab)
            .and_then(|t| indicator_of(&t.attention, &t.running))
    }

    /// What the quick terminal shows.
    pub fn quick_indicator(&self) -> Option<Indicator> {
        self.workspace
            .quick_terminal_raw()
            .and_then(|q| indicator_of(&q.attention, &q.running))
    }

    /// What a task card shows: the most important state among the threads linked to the
    /// task and, at any depth, to its subtasks.
    pub fn task_indicator(&self, task: &v1::Task) -> Option<Indicator> {
        self.linked_indicator(&task.thread_id, &task.subtasks)
    }

    /// Like [`Self::task_indicator`], for a subtask and its own subtasks.
    pub fn subtask_indicator(&self, subtask: &v1::Subtask) -> Option<Indicator> {
        self.linked_indicator(&subtask.thread_id, &subtask.subtasks)
    }

    fn linked_indicator(&self, thread: &str, subtasks: &[v1::Subtask]) -> Option<Indicator> {
        let mut threads = vec![thread];
        linked_threads(subtasks, &mut threads);
        threads
            .into_iter()
            .filter(|t| !t.is_empty())
            .filter_map(|t| self.thread_indicator(t))
            .max()
    }

    /// Whether any pane's agent is working, so its indicator pulses and the window keeps
    /// redrawing.
    pub fn any_running(&self) -> bool {
        self.workspace
            .raw()
            .tabs
            .iter()
            .any(|t| !t.running.is_empty())
            || self
                .workspace
                .quick_terminal_raw()
                .is_some_and(|q| !q.running.is_empty())
    }
}
