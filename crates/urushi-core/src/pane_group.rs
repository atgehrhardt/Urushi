//! A pane group: a split-pane layout plus which pane has focus.
//!
//! Tabs and the quick terminal both own one, so the layout operations and the focus
//! rule are written once.

use crate::PaneId;
use crate::layout::{Axis, Direction, Layout, LayoutError};

/// A layout of panes with one focused pane.
///
/// Invariant: `focused` is always a pane in `layout`.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneGroup {
    layout: Layout,
    focused: PaneId,
}

impl PaneGroup {
    /// A group of one pane, which has focus.
    pub fn new(first_pane: PaneId) -> Self {
        Self {
            layout: Layout::single(first_pane),
            focused: first_pane,
        }
    }

    /// Rebuilds a group from its parts (e.g. loaded or received over the wire). Fails if
    /// `focused` is not in `layout`.
    pub fn from_parts(layout: Layout, focused: PaneId) -> Result<Self, LayoutError> {
        if !layout.contains(focused) {
            return Err(LayoutError::PaneNotFound(focused));
        }
        Ok(Self { layout, focused })
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
    /// Returns [`LayoutError::LastPane`] for the last pane; close the whole group instead.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: u128) -> PaneId {
        PaneId::from_u128(n)
    }

    #[test]
    fn split_focuses_new_pane_and_close_restores_focus() {
        let mut g = PaneGroup::new(p(1));
        g.split(p(1), p(2), Direction::Right).unwrap();
        assert_eq!(g.focused(), p(2));
        g.close_pane(p(2)).unwrap();
        assert_eq!(g.focused(), p(1));
        assert_eq!(g.close_pane(p(1)), Err(LayoutError::LastPane));
    }

    #[test]
    fn closing_unfocused_pane_keeps_focus() {
        let mut g = PaneGroup::new(p(1));
        g.split(p(1), p(2), Direction::Right).unwrap();
        g.split(p(2), p(3), Direction::Down).unwrap();
        g.focus(p(1)).unwrap();
        g.close_pane(p(3)).unwrap();
        assert_eq!(g.focused(), p(1));
    }

    #[test]
    fn focus_towards_moves_spatially() {
        let mut g = PaneGroup::new(p(1));
        g.split(p(1), p(2), Direction::Right).unwrap();
        assert!(g.focus_towards(Direction::Left));
        assert_eq!(g.focused(), p(1));
        assert!(!g.focus_towards(Direction::Left));
        assert_eq!(g.focus(p(9)), Err(LayoutError::PaneNotFound(p(9))));
    }

    #[test]
    fn parts_must_focus_a_pane_of_the_layout() {
        assert!(PaneGroup::from_parts(Layout::single(p(1)), p(2)).is_err());
        assert_eq!(
            PaneGroup::from_parts(Layout::single(p(1)), p(1)),
            Ok(PaneGroup::new(p(1)))
        );
    }
}
