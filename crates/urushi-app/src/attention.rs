//! Attention as the app sees it: which threads and tabs have a pane whose agent finished
//! or waits for the user (ADR 0017), and which raises the app has already announced or
//! cleared. The daemon owns attention; it arrives on each tab of the workspace.

use std::collections::HashSet;

use urushi_client::v1;

use crate::state::AppState;
use crate::surface::Surface;

/// One raise of attention: a pane, and when it was raised. A new raise on the same pane
/// has a new time.
pub type Raise = (String, i64);

/// What the app has done about raises, so each is announced and cleared at most once,
/// and whether the user can see the window at all.
#[derive(Debug, Default)]
pub struct AttentionTracker {
    /// The window lost keyboard focus: nothing in it counts as seen, and new raises are
    /// announced with a system notification.
    pub in_background: bool,
    /// Raises already announced (or there before the app connected, which aren't).
    pub announced: HashSet<Raise>,
    /// Raises the app has asked the daemon to clear.
    pub cleared: HashSet<Raise>,
}

impl AttentionTracker {
    /// Forgets raises that are no longer in the workspace.
    pub fn retain(&mut self, current: &HashSet<Raise>) {
        self.announced.retain(|r| current.contains(r));
        self.cleared.retain(|r| current.contains(r));
    }
}

/// How urgent a kind is, for picking a thread's most urgent pane. An unknown kind (from
/// a newer daemon) counts as waiting.
pub fn urgency(kind: v1::AttentionKind) -> u8 {
    match kind {
        v1::AttentionKind::Done => 1,
        v1::AttentionKind::Waiting | v1::AttentionKind::Unspecified => 2,
    }
}

/// What the user is told, e.g. "Claude finished".
pub fn describe(kind: v1::AttentionKind, tab_title: &str) -> String {
    let who = if tab_title.is_empty() {
        "The terminal"
    } else {
        tab_title
    };
    match kind {
        v1::AttentionKind::Done => format!("{who} finished"),
        v1::AttentionKind::Waiting | v1::AttentionKind::Unspecified => {
            format!("{who} needs your input")
        }
    }
}

impl AppState {
    /// The most urgent attention among a thread's panes.
    pub fn thread_attention(&self, thread: &str) -> Option<v1::AttentionKind> {
        self.workspace
            .raw()
            .tabs
            .iter()
            .filter(|t| t.thread_id == thread)
            .flat_map(|t| &t.attention)
            .map(v1::PaneAttention::kind)
            .max_by_key(|k| urgency(*k))
    }

    /// The most urgent attention among a tab's panes.
    pub fn tab_attention(&self, tab: &str) -> Option<v1::AttentionKind> {
        self.workspace
            .raw()
            .tabs
            .iter()
            .find(|t| t.id == tab)
            .into_iter()
            .flat_map(|t| &t.attention)
            .map(v1::PaneAttention::kind)
            .max_by_key(|k| urgency(*k))
    }

    /// The most urgent attention among the quick terminal's panes.
    pub fn quick_attention(&self) -> Option<v1::AttentionKind> {
        self.workspace
            .quick_terminal_raw()
            .into_iter()
            .flat_map(|q| &q.attention)
            .map(v1::PaneAttention::kind)
            .max_by_key(|k| urgency(*k))
    }

    /// The most urgent attention among the threads linked to a task and, at any depth,
    /// to its subtasks: a card shows when any of its agents finished or waits.
    pub fn task_attention(&self, task: &v1::Task) -> Option<v1::AttentionKind> {
        self.linked_attention(&task.thread_id, &task.subtasks)
    }

    /// Like [`Self::task_attention`], for a subtask and its own subtasks.
    pub fn subtask_attention(&self, subtask: &v1::Subtask) -> Option<v1::AttentionKind> {
        self.linked_attention(&subtask.thread_id, &subtask.subtasks)
    }

    fn linked_attention(
        &self,
        thread: &str,
        subtasks: &[v1::Subtask],
    ) -> Option<v1::AttentionKind> {
        let mut threads = vec![thread];
        linked_threads(subtasks, &mut threads);
        threads
            .into_iter()
            .filter(|t| !t.is_empty())
            .filter_map(|t| self.thread_attention(t))
            .max_by_key(|k| urgency(*k))
    }

    /// Panes the user can see right now, while the window has focus and no dialog
    /// covers them: the quick terminal's while its panel is down, else the current
    /// tab's unless something covers them.
    pub fn visible_panes(&self) -> Vec<String> {
        if self.attention.in_background || self.dialog.is_some() {
            return Vec::new();
        }
        let surface = if self.quick.open {
            Surface::Quick
        } else if self.terminals_covered() || (self.compact && self.sidebar_open) {
            return Vec::new();
        } else {
            Surface::Main
        };
        self.pane_rects_of(surface)
            .into_iter()
            .map(|(pane, _)| pane)
            .collect()
    }
}

/// The threads linked to `subtasks` and theirs, at any depth.
pub(crate) fn linked_threads<'a>(subtasks: &'a [v1::Subtask], out: &mut Vec<&'a str>) {
    for s in subtasks {
        out.push(&s.thread_id);
        linked_threads(&s.subtasks, out);
    }
}
