//! Dragging a divider without waiting for the daemon (ADR 0056).
//!
//! The divider follows the pointer at once: the panes on screen are the daemon's layout
//! with the drag applied by `urushi_core::PaneGroup::move_divider`, the same rule the
//! daemon applies, so nothing is decided here. Moves are sent one at a time: while one
//! is unanswered, only the newest position waits, and it is sent when the answer comes.
//!
//! After the button is released and the last move answered, the preview stays until a
//! workspace from the daemon shows the divider where it was dropped, or, failing that
//! (another client moved it meanwhile), for one more workspace: the daemon announces a
//! change before it answers, so the second workspace after the answer is never older
//! than it.

use urushi_core::{Axis, PaneGroup, PaneId};
use urushi_ui::Rect;

use crate::terminal;

/// How close (in a 1000-point square) the daemon's panes must be to the preview's for
/// the preview to have done its job.
const SETTLED: f32 = 0.5;

/// A divider being dragged, as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct DividerPreview {
    /// The tab (or quick terminal) whose divider it is.
    pub group: String,
    /// The pane before the divider.
    pub anchor: PaneId,
    /// The split's axis.
    pub axis: Axis,
    /// Where the divider is, as a fraction of the split.
    pub position: f64,
}

/// The divider drag in progress, and the moves the daemon hasn't answered.
#[derive(Debug, Default)]
pub struct DividerDrag {
    preview: Option<DividerPreview>,
    /// A move is unanswered.
    in_flight: bool,
    /// The newest position, not sent yet because a move is unanswered.
    queued: Option<f64>,
    /// The button is up.
    released: bool,
    /// Workspaces seen since the last answer, once released.
    workspaces_after: u8,
}

impl DividerDrag {
    /// The divider moved to `preview`. Returns the position to send now, if any.
    pub fn moved(&mut self, preview: DividerPreview) -> Option<f64> {
        let position = preview.position;
        let restarted = self
            .preview
            .as_ref()
            .is_none_or(|p| p.group != preview.group || p.anchor != preview.anchor);
        if restarted {
            self.released = false;
            self.workspaces_after = 0;
        }
        self.preview = Some(preview);
        if self.in_flight {
            self.queued = Some(position);
            None
        } else {
            self.in_flight = true;
            Some(position)
        }
    }

    /// The daemon answered a move. Returns the next position to send, if one waits.
    pub fn answered(&mut self) -> Option<f64> {
        self.workspaces_after = 0;
        let next = self.queued.take();
        if next.is_none() {
            self.in_flight = false;
        }
        next
    }

    /// The button was released.
    pub fn released(&mut self) {
        self.released = true;
    }

    /// A workspace came from the daemon; `group` is its pane group for the dragged
    /// divider. Drops the preview once it is no longer needed.
    pub fn workspace(&mut self, group: Option<&PaneGroup>) {
        if !self.released || self.in_flight {
            return;
        }
        let Some(preview) = &self.preview else {
            return;
        };
        self.workspaces_after = self.workspaces_after.saturating_add(1);
        let settled = group.is_none_or(|g| matches(g, preview));
        if settled || self.workspaces_after >= 2 {
            self.preview = None;
        }
    }

    /// `group` (the daemon's layout of `id`) as it should show: with the divider where
    /// it is being dragged.
    #[must_use]
    pub fn apply(&self, id: &str, mut group: PaneGroup) -> PaneGroup {
        if let Some(p) = self.preview.as_ref().filter(|p| p.group == id) {
            // A pane gone meanwhile just means the daemon's layout shows as it is.
            let _ = group.move_divider(p.anchor, p.axis, p.position);
        }
        group
    }

    /// The divider as shown, while it is.
    #[must_use]
    pub fn preview(&self) -> Option<&DividerPreview> {
        self.preview.as_ref()
    }

    /// The tab (or quick terminal) whose divider is shown moved, if any.
    #[must_use]
    pub fn group(&self) -> Option<&str> {
        self.preview.as_ref().map(|p| p.group.as_str())
    }
}

/// Whether `group` already has the divider where `preview` puts it.
fn matches(group: &PaneGroup, preview: &DividerPreview) -> bool {
    let area = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let mut moved = group.clone();
    if moved
        .move_divider(preview.anchor, preview.axis, preview.position)
        .is_err()
    {
        return true;
    }
    let before = terminal::pane_rects(group.layout(), area);
    let after = terminal::pane_rects(moved.layout(), area);
    before.len() == after.len()
        && before.iter().zip(&after).all(|((a, ra), (b, rb))| {
            a == b
                && (ra.x - rb.x).abs() < SETTLED
                && (ra.y - rb.y).abs() < SETTLED
                && (ra.width - rb.width).abs() < SETTLED
                && (ra.height - rb.height).abs() < SETTLED
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(position: f64) -> DividerPreview {
        DividerPreview {
            group: "tab".into(),
            anchor: PaneId::from_u128(1),
            axis: Axis::Row,
            position,
        }
    }

    #[test]
    fn one_move_at_a_time_and_only_the_newest_waits() {
        let mut d = DividerDrag::default();
        assert_eq!(d.moved(preview(0.3)), Some(0.3));
        assert_eq!(d.moved(preview(0.4)), None);
        assert_eq!(d.moved(preview(0.5)), None);
        assert_eq!(d.answered(), Some(0.5));
        assert_eq!(d.answered(), None);
        assert_eq!(d.moved(preview(0.6)), Some(0.6));
    }

    #[test]
    fn the_preview_stays_until_the_daemon_shows_the_drop() {
        let mut d = DividerDrag::default();
        d.moved(preview(0.3));
        d.released();
        d.workspace(None);
        assert_eq!(d.group(), Some("tab"), "a move is unanswered");
        d.answered();
        let a = PaneId::from_u128(1);
        let b = PaneId::from_u128(2);
        let mut stale = PaneGroup::new(a);
        stale.split(a, b, urushi_core::Direction::Right).unwrap();
        d.workspace(Some(&stale));
        assert_eq!(d.group(), Some("tab"), "the daemon's layout is still 50/50");
        let mut fresh = stale.clone();
        fresh.move_divider(a, Axis::Row, 0.3).unwrap();
        d.workspace(Some(&fresh));
        assert_eq!(d.group(), None);
    }

    #[test]
    fn the_preview_goes_after_two_workspaces_whatever_they_show() {
        let mut d = DividerDrag::default();
        d.moved(preview(0.3));
        d.answered();
        d.released();
        let a = PaneId::from_u128(1);
        let mut other = PaneGroup::new(a);
        other
            .split(a, PaneId::from_u128(2), urushi_core::Direction::Right)
            .unwrap();
        d.workspace(Some(&other));
        assert!(d.group().is_some());
        d.workspace(Some(&other));
        assert!(d.group().is_none());
    }
}
