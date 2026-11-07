//! A tab: one split-pane layout plus which pane has focus.

use serde::{Deserialize, Serialize};

use crate::layout::{Axis, Direction, Layout, LayoutError};
use crate::{PaneId, TabId, ThreadId};

/// A tab inside a thread.
///
/// Invariant: `focused` is always a pane in `layout`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TabData", into = "TabData")]
pub struct Tab {
    id: TabId,
    thread: ThreadId,
    title: String,
    layout: Layout,
    focused: PaneId,
}

impl Tab {
    /// A new tab containing one pane, which has focus.
    pub fn new(id: TabId, thread: ThreadId, title: impl Into<String>, first_pane: PaneId) -> Self {
        Self {
            id,
            thread,
            title: title.into(),
            layout: Layout::single(first_pane),
            focused: first_pane,
        }
    }

    /// Rebuilds a tab from its parts (e.g. received over the wire). Fails if `focused` is
    /// not in `layout`.
    pub fn from_parts(
        id: TabId,
        thread: ThreadId,
        title: impl Into<String>,
        layout: Layout,
        focused: PaneId,
    ) -> Result<Self, LayoutError> {
        if !layout.contains(focused) {
            return Err(LayoutError::PaneNotFound(focused));
        }
        Ok(Self {
            id,
            thread,
            title: title.into(),
            layout,
            focused,
        })
    }

    /// This tab's id.
    pub const fn id(&self) -> TabId {
        self.id
    }

    /// The thread this tab belongs to.
    pub const fn thread(&self) -> ThreadId {
        self.thread
    }

    /// Display title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Renames the tab.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// The pane layout.
    pub const fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The focused pane.
    pub const fn focused(&self) -> PaneId {
        self.focused
    }

    /// Focuses `pane`.
    pub fn focus(&mut self, pane: PaneId) -> Result<(), LayoutError> {
        if !self.layout.contains(pane) {
            return Err(LayoutError::PaneNotFound(pane));
        }
        self.focused = pane;
        Ok(())
    }

    /// Moves focus to the neighbouring pane towards `side`. Returns `false` at the edge.
    pub fn focus_towards(&mut self, side: Direction) -> bool {
        match self.layout.focus_neighbour(self.focused, side) {
            Some(pane) => {
                self.focused = pane;
                true
            }
            None => false,
        }
    }

    /// Splits `target`, adding `new` on `side`. The new pane gets focus.
    pub fn split(
        &mut self,
        target: PaneId,
        new: PaneId,
        side: Direction,
    ) -> Result<(), LayoutError> {
        self.layout.split(target, new, side)?;
        self.focused = new;
        Ok(())
    }

    /// Closes `pane`. If it had focus, focus moves to the pane that took its space.
    /// Returns [`LayoutError::LastPane`] for the last pane; close the tab instead.
    pub fn close_pane(&mut self, pane: PaneId) -> Result<(), LayoutError> {
        let successor = self.layout.close(pane)?;
        if self.focused == pane {
            self.focused = successor;
        }
        Ok(())
    }

    /// See [`Layout::resize`].
    pub fn resize(
        &mut self,
        pane: PaneId,
        side: Direction,
        amount: f64,
    ) -> Result<(), LayoutError> {
        self.layout.resize(pane, side, amount)
    }

    /// See [`Layout::move_divider`].
    pub fn move_divider(
        &mut self,
        anchor: PaneId,
        axis: Axis,
        position: f64,
    ) -> Result<(), LayoutError> {
        self.layout.move_divider(anchor, axis, position)
    }

    /// See [`Layout::equalize`].
    pub fn equalize(&mut self) {
        self.layout.equalize();
    }
}

/// Serialized form of [`Tab`]. Checked on load so the focus invariant holds.
#[derive(Serialize, Deserialize)]
struct TabData {
    id: TabId,
    thread: ThreadId,
    title: String,
    layout: Layout,
    focused: PaneId,
}

impl TryFrom<TabData> for Tab {
    type Error = LayoutError;

    fn try_from(d: TabData) -> Result<Self, Self::Error> {
        Self::from_parts(d.id, d.thread, d.title, d.layout, d.focused)
    }
}

impl From<Tab> for TabData {
    fn from(t: Tab) -> Self {
        Self {
            id: t.id,
            thread: t.thread,
            title: t.title,
            layout: t.layout,
            focused: t.focused,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: u128) -> PaneId {
        PaneId::from_u128(n)
    }

    fn tab() -> Tab {
        Tab::new(TabId::from_u128(1), ThreadId::from_u128(1), "shell", p(1))
    }

    #[test]
    fn split_focuses_new_pane_and_close_restores_focus() {
        let mut t = tab();
        t.split(p(1), p(2), Direction::Right).unwrap();
        assert_eq!(t.focused(), p(2));
        t.close_pane(p(2)).unwrap();
        assert_eq!(t.focused(), p(1));
        assert_eq!(t.close_pane(p(1)), Err(LayoutError::LastPane));
    }

    #[test]
    fn closing_unfocused_pane_keeps_focus() {
        let mut t = tab();
        t.split(p(1), p(2), Direction::Right).unwrap();
        t.split(p(2), p(3), Direction::Down).unwrap();
        t.focus(p(1)).unwrap();
        t.close_pane(p(3)).unwrap();
        assert_eq!(t.focused(), p(1));
    }

    #[test]
    fn focus_towards_moves_spatially() {
        let mut t = tab();
        t.split(p(1), p(2), Direction::Right).unwrap();
        assert!(t.focus_towards(Direction::Left));
        assert_eq!(t.focused(), p(1));
        assert!(!t.focus_towards(Direction::Left));
        assert_eq!(t.focus(p(9)), Err(LayoutError::PaneNotFound(p(9))));
    }

    #[test]
    fn deserialization_rejects_dangling_focus() {
        let mut json = serde_json::to_value(tab()).unwrap();
        json["focused"] = serde_json::json!(p(7));
        assert!(serde_json::from_value::<Tab>(json).is_err());
        let ok: Tab = serde_json::from_value(serde_json::to_value(tab()).unwrap()).unwrap();
        assert_eq!(ok, tab());
    }
}
