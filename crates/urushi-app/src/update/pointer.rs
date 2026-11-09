//! The mouse over the terminal area: dividers, text selection, the wheel, the pane
//! menu, and mouse reporting to applications that ask for the mouse (Claude Code, Codex,
//! vim, …).
//!
//! A right or two-finger click always opens the pane's context menu
//! (`update/panes.rs`). When a pane's application has mouse reporting on, presses,
//! drags, releases, hover, the middle button and the wheel go to it as `MouseInput` /
//! `WheelInput`; the daemon encodes them (or, for the wheel, turns them into arrow keys
//! or scrollback). Holding Shift selects text instead, as in other terminals. Daemons
//! older than protocol 1.3 get the old behaviour: selection, and `ScrollPane` for the
//! wheel.

use urushi_client::v1::{self, notification, request::Body};
use urushi_ui::{Point, Rect};

use super::{input, request};
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Drag, Selection};
use crate::terminal;

/// First protocol minor version that understands mouse and wheel input.
const MOUSE_SINCE_MINOR: u32 = 3;

impl AppState {
    /// Handles the pointer messages (see the arm in `update`).
    pub(super) fn pointer(&mut self, msg: &Msg, fx: &mut Vec<Effect>) {
        match *msg {
            Msg::AreaPress(p) => self.area_press(p, fx),
            Msg::AreaDrag(p) => self.area_drag(p, fx),
            Msg::AreaRelease(p) => self.area_release(p, fx),
            Msg::AreaWheel(p, lines) => self.area_wheel(p, lines, fx),
            Msg::AreaHover(p) => self.area_hover(p, fx),
            Msg::AreaButton {
                at,
                button,
                pressed,
            } => self.area_button(at, button, pressed, fx),
            Msg::Modifiers(mods) => self.mods = mods,
            _ => {}
        }
    }

    fn pane_at(&self, p: Point) -> Option<(String, Rect)> {
        self.pane_rects().into_iter().find(|(_, r)| r.contains(p))
    }

    /// The cell under `p` in `pane` (clamped to the grid), as `(col, row)`.
    fn cell_in(&self, pane: &str, p: Point) -> Option<(u16, u16)> {
        let (_, rect) = self.pane_rects().into_iter().find(|(id, _)| id == pane)?;
        let screen = self.screens.get(pane)?;
        let (row, col) = terminal::cell_at(rect, self.cell, screen, p);
        Some((col, row))
    }

    /// Whether clicks in `pane` go to its application rather than selecting text.
    fn reports_mouse(&self, pane: &str) -> bool {
        self.daemon_minor >= MOUSE_SINCE_MINOR
            && !self.mods.shift
            && self
                .screens
                .get(pane)
                .is_some_and(|s| s.modes.mouse_reporting)
    }

    fn mouse(
        &self,
        pane: &str,
        p: Point,
        button: v1::MouseButton,
        action: v1::MouseAction,
        fx: &mut Vec<Effect>,
    ) {
        if let Some((col, row)) = self.cell_in(pane, p) {
            let event = v1::MouseInput {
                button: button.into(),
                action: action.into(),
                col: col.into(),
                row: row.into(),
                modifiers: self.mods.bits(),
            };
            fx.push(input(pane.to_owned(), v1::input::Kind::Mouse(event)));
        }
    }

    fn area_press(&mut self, p: Point, fx: &mut Vec<Effect>) {
        if self.dialog.is_some() {
            return;
        }
        let (Some(area), Some(tab_id)) = (self.area, self.current_tab().map(|t| t.id.clone()))
        else {
            return;
        };
        if let Some(tab) = self.workspace.tab(&tab_id)
            && let Some(hit) = terminal::divider_at(tab.layout(), area, p)
        {
            self.drag = Some(Drag::Divider {
                tab: tab_id,
                anchor: hit.anchor,
                axis: hit.axis,
                split: hit.split,
            });
            return;
        }
        let Some((pane, rect)) = self.pane_at(p) else {
            return;
        };
        self.focus_pane(&tab_id, &pane, fx);
        if self.reports_mouse(&pane) {
            self.selection = None;
            let button = v1::MouseButton::Left;
            self.hover = self.cell_in(&pane, p).map(|c| (pane.clone(), c));
            self.mouse(&pane, p, button, v1::MouseAction::Press, fx);
            self.drag = Some(Drag::Report { pane, button });
        } else if let Some(screen) = self.screens.get(&pane) {
            let cell = terminal::cell_at(rect, self.cell, screen, p);
            self.selection = Some((
                pane.clone(),
                Selection {
                    anchor: cell,
                    head: cell,
                },
            ));
            self.drag = Some(Drag::Select { pane });
        }
    }

    fn area_drag(&mut self, p: Point, fx: &mut Vec<Effect>) {
        match self.drag.clone() {
            Some(Drag::Divider {
                tab,
                anchor,
                axis,
                split,
            }) => {
                let position = terminal::divider_position(axis, split, p);
                let axis = urushi_proto::convert::axis_to_proto(axis);
                let body = Body::MoveDivider(v1::MoveDividerRequest {
                    tab_id: tab,
                    anchor_pane_id: anchor,
                    axis: axis.into(),
                    position,
                });
                fx.push(request(body, Followup::None));
            }
            Some(Drag::Select { pane }) => {
                if let Some((col, row)) = self.cell_in(&pane, p)
                    && let Some((_, selection)) = &mut self.selection
                {
                    selection.head = (row, col);
                }
            }
            Some(Drag::Report { pane, button }) => {
                let cell = self.cell_in(&pane, p);
                if cell.is_some() && self.hover.as_ref().map(|(_, c)| *c) != cell {
                    self.hover = cell.map(|c| (pane.clone(), c));
                    self.mouse(&pane, p, button, v1::MouseAction::Move, fx);
                }
            }
            None => {}
        }
    }

    fn area_release(&mut self, p: Point, fx: &mut Vec<Effect>) {
        if let Some(Drag::Report { pane, button }) = self.drag.take() {
            self.mouse(&pane, p, button, v1::MouseAction::Release, fx);
        }
        if self
            .selection
            .as_ref()
            .is_some_and(|(_, s)| s.anchor == s.head)
        {
            self.selection = None;
        }
    }

    /// Movement with no button: reported once per cell, to applications that want it
    /// (the daemon drops it unless the app asked for all motion).
    fn area_hover(&mut self, p: Point, fx: &mut Vec<Effect>) {
        if self.dialog.is_some() || self.menu.is_some() {
            return;
        }
        let Some((pane, _)) = self.pane_at(p) else {
            self.hover = None;
            return;
        };
        if !self.reports_mouse(&pane) {
            return;
        }
        let cell = self.cell_in(&pane, p);
        let current = cell.map(|c| (pane.clone(), c));
        if cell.is_some() && self.hover != current {
            self.hover = current;
            self.mouse(&pane, p, v1::MouseButton::None, v1::MouseAction::Move, fx);
        }
    }

    /// A right (or two-finger) click opens the pane's menu, even over an application
    /// that takes the mouse. The middle button is reported to applications that do.
    fn area_button(
        &mut self,
        p: Point,
        button: v1::MouseButton,
        pressed: bool,
        fx: &mut Vec<Effect>,
    ) {
        if self.dialog.is_some() {
            return;
        }
        let Some((pane, _)) = self.pane_at(p) else {
            return;
        };
        if button == v1::MouseButton::Right {
            if pressed {
                self.last_press = Some(p);
                self.open_pane_menu(pane, fx);
            }
        } else if self.reports_mouse(&pane) {
            let action = if pressed {
                v1::MouseAction::Press
            } else {
                v1::MouseAction::Release
            };
            self.mouse(&pane, p, button, action, fx);
        }
    }

    /// The wheel: positive `lines` is towards older content.
    fn area_wheel(&self, p: Point, lines: i32, fx: &mut Vec<Effect>) {
        let Some((pane, _)) = self.pane_at(p) else {
            return;
        };
        if self.daemon_minor < MOUSE_SINCE_MINOR {
            fx.push(Effect::Notify(notification::Body::ScrollPane(
                v1::ScrollPane {
                    pane_id: pane,
                    lines,
                    to_bottom: false,
                },
            )));
            return;
        }
        let (col, row) = self.cell_in(&pane, p).unwrap_or((0, 0));
        let wheel = v1::WheelInput {
            up: lines > 0,
            lines: lines.unsigned_abs(),
            col: col.into(),
            row: row.into(),
            modifiers: self.mods.bits(),
        };
        fx.push(input(pane, v1::input::Kind::Wheel(wheel)));
    }
}
