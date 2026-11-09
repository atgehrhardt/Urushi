//! Attention: which panes' programs finished or wait for the user (ADR 0017).
//!
//! Each session's watcher raises attention when the pane's program rings the bell,
//! posts a terminal notification, or sends Urushi's signal (agent hooks). It is cleared
//! when someone types into the pane or a client says the user is looking at it
//! (`ClearAttention`). It lives only in memory: it isn't part of the stored workspace
//! and doesn't change its revision, but every change is broadcast as `WorkspaceChanged`,
//! with each tab listing its panes' attention.

use urushi_core::{AttentionKind, PaneId, Timestamp};
use urushi_proto::v1;

use super::state::State;
use super::{Service, lock, now};

/// A pane's attention, since when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attention {
    pub(crate) kind: AttentionKind,
    pub(crate) since: Timestamp,
}

impl Service {
    /// Raises `kind` on `pane`. The same kind again changes nothing (a program ringing
    /// the bell repeatedly is one alert until the user looks).
    pub(crate) fn raise_attention(&self, pane: PaneId, kind: AttentionKind) {
        {
            let mut attention = lock(&self.attention);
            if attention.get(&pane).is_some_and(|a| a.kind == kind) {
                return;
            }
            attention.insert(pane, Attention { kind, since: now() });
        }
        self.broadcast();
    }

    /// Clears the attention of `panes` (the user typed or looked). Broadcasts only if
    /// something was cleared.
    pub fn clear_attention(&self, panes: impl IntoIterator<Item = PaneId>) {
        let cleared = {
            let mut attention = lock(&self.attention);
            if attention.is_empty() {
                return;
            }
            panes
                .into_iter()
                .fold(false, |any, pane| attention.remove(&pane).is_some() || any)
        };
        if cleared {
            self.broadcast();
        }
    }

    /// The attention of `pane`, if any.
    pub fn attention(&self, pane: PaneId) -> Option<AttentionKind> {
        lock(&self.attention).get(&pane).map(|a| a.kind)
    }

    /// Forgets panes that are gone. Their removal is broadcast by whoever removed them.
    pub(super) fn forget_attention(&self, panes: &[PaneId]) {
        let mut attention = lock(&self.attention);
        for pane in panes {
            attention.remove(pane);
        }
    }

    /// Adds each tab's attention to `workspace`, which was made from `state` (same tab
    /// order).
    pub(super) fn add_attention(&self, workspace: &mut v1::Workspace, state: &State) {
        let attention = lock(&self.attention);
        if attention.is_empty() {
            return;
        }
        for (wire, tab) in workspace.tabs.iter_mut().zip(&state.tabs) {
            wire.attention = tab
                .layout()
                .panes()
                .into_iter()
                .filter_map(|pane| {
                    let a = attention.get(&pane)?;
                    Some(v1::PaneAttention {
                        pane_id: pane.to_string(),
                        kind: kind_to_proto(a.kind).into(),
                        since_ms: i64::try_from(a.since.unix_millis()).unwrap_or(i64::MAX),
                    })
                })
                .collect();
        }
    }

    /// Wakes subscribers without a new revision: attention isn't stored.
    fn broadcast(&self) {
        let revision = lock(&self.state).revision;
        self.changes.send_replace(revision);
    }
}

fn kind_to_proto(kind: AttentionKind) -> v1::AttentionKind {
    match kind {
        AttentionKind::Done => v1::AttentionKind::Done,
        AttentionKind::Waiting => v1::AttentionKind::Waiting,
    }
}
