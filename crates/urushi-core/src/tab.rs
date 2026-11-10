//! A tab: a thread's titled pane group.

use serde::{Deserialize, Serialize};

use crate::layout::{Axis, Direction, Layout, LayoutError};
use crate::pane_group::PaneGroup;
use crate::{PaneId, TabId, ThreadId};

/// A tab inside a thread: a title and a [`PaneGroup`].
///
/// Invariant: the focused pane is always a pane in the layout (kept by [`PaneGroup`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TabData", into = "TabData")]
pub struct Tab {
    id: TabId,
    thread: ThreadId,
    title: String,
    group: PaneGroup,
}

impl Tab {
    /// A new tab containing one pane, which has focus.
    pub fn new(id: TabId, thread: ThreadId, title: impl Into<String>, first_pane: PaneId) -> Self {
        Self {
            id,
            thread,
            title: title.into(),
            group: PaneGroup::new(first_pane),
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
        Ok(Self {
            id,
            thread,
            title: title.into(),
            group: PaneGroup::from_parts(layout, focused)?,
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

    /// The tab's panes and focus.
    pub const fn group(&self) -> &PaneGroup {
        &self.group
    }

    /// The tab's panes and focus, for layout operations shared with other pane groups.
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

    /// See [`PaneGroup::focus`].
    pub fn focus(&mut self, pane: PaneId) -> Result<(), LayoutError> {
        self.group.focus(pane)
    }

    /// See [`PaneGroup::focus_towards`].
    pub fn focus_towards(&mut self, side: Direction) -> bool {
        self.group.focus_towards(side)
    }

    /// See [`PaneGroup::split`].
    pub fn split(
        &mut self,
        target: PaneId,
        new: PaneId,
        side: Direction,
    ) -> Result<(), LayoutError> {
        self.group.split(target, new, side)
    }

    /// See [`PaneGroup::close_pane`]. For the last pane, close the tab instead.
    pub fn close_pane(&mut self, pane: PaneId) -> Result<(), LayoutError> {
        self.group.close_pane(pane)
    }

    /// See [`Layout::resize`].
    pub fn resize(
        &mut self,
        pane: PaneId,
        side: Direction,
        amount: f64,
    ) -> Result<(), LayoutError> {
        self.group.resize(pane, side, amount)
    }

    /// See [`Layout::move_divider`].
    pub fn move_divider(
        &mut self,
        anchor: PaneId,
        axis: Axis,
        position: f64,
    ) -> Result<(), LayoutError> {
        self.group.move_divider(anchor, axis, position)
    }

    /// See [`Layout::equalize`].
    pub fn equalize(&mut self) {
        self.group.equalize();
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
            focused: t.group.focused(),
            layout: t.group.layout().clone(),
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
    fn a_tab_acts_on_its_pane_group() {
        let mut t = tab();
        t.split(p(1), p(2), Direction::Right).unwrap();
        assert_eq!(t.focused(), p(2));
        assert_eq!(t.group().layout().len(), 2);
        t.group_mut().focus(p(1)).unwrap();
        assert_eq!(t.focused(), p(1));
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

    /// `workspace.json` keeps the same tab keys it always had (ADR 0008).
    #[test]
    fn the_stored_form_is_unchanged() {
        let json = serde_json::to_value(tab()).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["focused", "id", "layout", "thread", "title"]);
    }
}
