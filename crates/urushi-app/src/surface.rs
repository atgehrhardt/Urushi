//! Where terminals are on screen, and which of them gets the keyboard and the mouse.
//!
//! There are two surfaces: the main area, showing the current tab's panes, and the quick
//! terminal's drop-down panel over it (ADR 0031). Both are pane groups the daemon names
//! by a tab id, so splitting, closing and focusing work the same on either.

use urushi_core::PaneGroup;
use urushi_ui::{Point, Rect};

use crate::state::AppState;
use crate::terminal::{self, DividerHit};

/// A place where panes are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// The main area: the selected thread's current tab.
    Main,
    /// The quick terminal's drop-down panel.
    Quick,
}

impl AppState {
    /// The id (a tab id, or the quick terminal's) and panes shown on `surface`.
    pub fn surface_group(&self, surface: Surface) -> Option<(String, PaneGroup)> {
        match surface {
            Surface::Main => {
                let id = self.current_tab()?.id.clone();
                let group = self.workspace.group(&id)?;
                Some((id, group))
            }
            Surface::Quick => {
                let quick = self.workspace.quick_terminal()?;
                Some((quick.id().to_string(), quick.group().clone()))
            }
        }
    }

    /// The id of what `surface` shows (a tab id, or the quick terminal's), as sent.
    pub fn surface_id(&self, surface: Surface) -> Option<String> {
        match surface {
            Surface::Main => self.current_tab().map(|t| t.id.clone()),
            Surface::Quick => self.workspace.quick_terminal_raw().map(|q| q.id.clone()),
        }
    }

    /// The surface whose panes typing, splitting and closing act on: the quick
    /// terminal while its panel is down, else the main area.
    pub fn active_surface(&self) -> Surface {
        if self.quick.open {
            Surface::Quick
        } else {
            Surface::Main
        }
    }

    /// The surface that takes typing now, if any: none under a dialog, the quick
    /// terminal while its panel is down (above the board), none under the board.
    pub fn keyboard_surface(&self) -> Option<Surface> {
        if self.dialog.is_some() {
            None
        } else if self.quick.open {
            Some(Surface::Quick)
        } else if self.board.visible() {
            None
        } else {
            Some(Surface::Main)
        }
    }

    /// The surface whose terminals take the mouse at `p`, if any. While the panel is
    /// down, only its own terminals do; below it is the strip that puts it away.
    pub fn surface_at(&self, p: Point) -> Option<Surface> {
        if self.dialog.is_some() {
            return None;
        }
        if self.quick.visible() {
            let inside = self.quick.area.is_some_and(|a| a.contains(p));
            return (self.quick.open && inside).then_some(Surface::Quick);
        }
        let inside = self.area.is_some_and(|a| a.contains(p));
        (inside && !self.terminals_covered()).then_some(Surface::Main)
    }

    /// Whether the pointer at `p` is over terminals that take the mouse.
    pub fn terminal_at(&self, p: Point) -> bool {
        self.surface_at(p).is_some()
    }

    /// The divider under `p`, in whichever surface takes the mouse there.
    pub fn divider_at(&self, p: Point) -> Option<DividerHit> {
        let surface = self.surface_at(p)?;
        let (_, group) = self.surface_group(surface)?;
        terminal::divider_at(group.layout(), self.surface_area(surface)?, p)
    }

    /// The focused pane of the active surface: the pane focused here if the daemon
    /// hasn't confirmed it yet and it still exists, otherwise the daemon's choice.
    pub fn focused_pane(&self) -> Option<String> {
        self.focused_in(&self.surface_id(self.active_surface())?)
    }

    /// The focused pane of the tab (or quick terminal) `id`, as for
    /// [`Self::focused_pane`].
    pub fn focused_in(&self, id: &str) -> Option<String> {
        let confirmed = self.confirmed_focus(id)?;
        Some(
            self.focused_override
                .get(id)
                .filter(|pane| self.group_has_pane(id, pane))
                .cloned()
                .unwrap_or(confirmed),
        )
    }

    /// The pane the daemon says has focus in the tab (or quick terminal) `id`.
    pub fn confirmed_focus(&self, id: &str) -> Option<String> {
        let raw = self.workspace.raw();
        raw.tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.focused_pane_id.clone())
            .or_else(|| {
                raw.quick_terminal
                    .as_ref()
                    .filter(|q| q.id == id)
                    .map(|q| q.focused_pane_id.clone())
            })
    }

    /// Whether `pane` is one of the panes of the tab (or quick terminal) `id`.
    pub fn group_has_pane(&self, id: &str, pane: &str) -> bool {
        pane.parse::<urushi_core::PaneId>().is_ok_and(|pane| {
            self.workspace
                .group(id)
                .is_some_and(|g| g.layout().contains(pane))
        })
    }

    /// Whether something covers the main area's terminals (a dialog, the board, or the
    /// quick terminal's panel), so the mouse and keyboard don't reach them.
    pub fn terminals_covered(&self) -> bool {
        self.dialog.is_some() || self.board.visible() || self.quick.visible()
    }

    /// Panes on screen, with their rectangles: the current tab's, and the quick
    /// terminal's while its panel is out. These are the panes kept attached.
    pub fn pane_rects(&self) -> Vec<(String, Rect)> {
        let mut rects = self.pane_rects_of(Surface::Main);
        rects.extend(self.pane_rects_of(Surface::Quick));
        rects
    }

    /// Panes on `surface`, with their rectangles.
    pub fn pane_rects_of(&self, surface: Surface) -> Vec<(String, Rect)> {
        if surface == Surface::Quick && !self.quick.visible() {
            return Vec::new();
        }
        match (self.surface_area(surface), self.surface_group(surface)) {
            (Some(area), Some((_, group))) => terminal::pane_rects(group.layout(), area),
            _ => Vec::new(),
        }
    }

    fn surface_area(&self, surface: Surface) -> Option<Rect> {
        match surface {
            Surface::Main => self.area,
            Surface::Quick => self.quick.area,
        }
    }
}
