//! Attention as the app sees it: which threads and tabs have a pane whose agent finished
//! or waits for the user (ADR 0017), and which raises the app has already announced or
//! cleared. The daemon owns attention; it arrives on each tab of the workspace.

use std::collections::HashSet;

use urushi_client::v1;

use crate::state::AppState;

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

    /// Panes the user can see right now: the current tab's, while the window has focus
    /// and nothing covers the terminals.
    pub fn visible_panes(&self) -> Vec<String> {
        if self.attention.in_background
            || self.terminals_covered()
            || (self.compact && self.sidebar_open)
        {
            return Vec::new();
        }
        self.pane_rects()
            .into_iter()
            .map(|(pane, _)| pane)
            .collect()
    }
}
