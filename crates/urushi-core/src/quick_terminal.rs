//! The quick terminal: split panes that belong to no thread (ADR 0027).
//!
//! A daemon has at most one. Clients show it as a drop-down panel over the workspace;
//! hiding it leaves its processes running. It ends when its last pane closes.

use serde::{Deserialize, Serialize};

use crate::layout::{Layout, LayoutError};
use crate::pane_group::PaneGroup;
use crate::{PaneId, TabId};

/// The daemon's thread-less pane group.
///
/// Its id is a [`TabId`] so every layout request that names a tab (split, close, focus,
/// dividers) can name the quick terminal too. Ids are unique, so it never collides
/// with a real tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "QuickTerminalData", into = "QuickTerminalData")]
pub struct QuickTerminal {
    id: TabId,
    group: PaneGroup,
}

impl QuickTerminal {
    /// A quick terminal with one pane, which has focus.
    pub fn new(id: TabId, first_pane: PaneId) -> Self {
        Self {
            id,
            group: PaneGroup::new(first_pane),
        }
    }

    /// Rebuilds a quick terminal from its parts. Fails if `focused` is not in `layout`.
    pub fn from_parts(id: TabId, layout: Layout, focused: PaneId) -> Result<Self, LayoutError> {
        Ok(Self {
            id,
            group: PaneGroup::from_parts(layout, focused)?,
        })
    }

    /// Its id, accepted wherever a layout request takes a tab id.
    pub const fn id(&self) -> TabId {
        self.id
    }

    /// Its panes and focus.
    pub const fn group(&self) -> &PaneGroup {
        &self.group
    }

    /// Its panes and focus, for layout operations.
    pub const fn group_mut(&mut self) -> &mut PaneGroup {
        &mut self.group
    }

    /// The pane layout.
    pub const fn layout(&self) -> &Layout {
        self.group.layout()
    }

    /// The focused pane.
    pub const fn focused(&self) -> PaneId {
        self.group.focused()
    }
}

/// Serialized form of [`QuickTerminal`]. Checked on load so the focus invariant holds.
#[derive(Serialize, Deserialize)]
struct QuickTerminalData {
    id: TabId,
    layout: Layout,
    focused: PaneId,
}

impl TryFrom<QuickTerminalData> for QuickTerminal {
    type Error = LayoutError;

    fn try_from(d: QuickTerminalData) -> Result<Self, Self::Error> {
        Self::from_parts(d.id, d.layout, d.focused)
    }
}

impl From<QuickTerminal> for QuickTerminalData {
    fn from(q: QuickTerminal) -> Self {
        Self {
            id: q.id,
            focused: q.group.focused(),
            layout: q.group.layout().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Direction;

    fn p(n: u128) -> PaneId {
        PaneId::from_u128(n)
    }

    #[test]
    fn it_splits_like_a_tab_and_round_trips() {
        let mut q = QuickTerminal::new(TabId::from_u128(1), p(1));
        q.group_mut().split(p(1), p(2), Direction::Down).unwrap();
        assert_eq!(q.focused(), p(2));
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(serde_json::from_value::<QuickTerminal>(json).unwrap(), q);
    }

    #[test]
    fn loading_rejects_a_dangling_focus() {
        let mut json = serde_json::to_value(QuickTerminal::new(TabId::from_u128(1), p(1))).unwrap();
        json["focused"] = serde_json::json!(p(7));
        assert!(serde_json::from_value::<QuickTerminal>(json).is_err());
    }
}
