//! The desktop sidebar's width and visibility: collapsing it, dragging its edge, and the
//! slide in between.
//!
//! This is the window's own view state, so it lives in the app rather than the daemon
//! (ADR 0002), and `ui_state.rs` remembers it across launches. Phone-width windows
//! don't use it: they show the sidebar or the terminals, never both (`sidebar_open`).

/// Width of a sidebar nobody has resized.
pub const DEFAULT_WIDTH: f32 = 264.0;
/// Narrowest the sidebar can be dragged.
pub const MIN_WIDTH: f32 = 200.0;
/// Widest the sidebar can be dragged (it never takes more than half the window).
pub const MAX_WIDTH: f32 = 560.0;
/// Dragged narrower than this, the sidebar collapses; dragging back out reopens it.
pub const SNAP_WIDTH: f32 = 140.0;
/// How long collapsing or expanding takes.
pub const SLIDE_SECONDS: f32 = 0.18;

/// A drag of the sidebar's edge in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Resize {
    /// Pointer x minus the sidebar's width when the drag started, so the edge keeps
    /// its distance from the pointer wherever it was grabbed (and wherever the sidebar
    /// sits in the window).
    offset: f32,
}

/// Width, collapsed state and slide of the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarLayout {
    /// The chosen width (before fitting the window).
    width: f32,
    /// Hidden by the user.
    collapsed: bool,
    /// How far the sidebar is out: 1 shown, 0 hidden. Animated towards `!collapsed`.
    reveal: f32,
    /// The drag in progress.
    resize: Option<Resize>,
    /// The window's width in logical pixels (0 until known).
    window: f32,
}

impl Default for SidebarLayout {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            collapsed: false,
            reveal: 1.0,
            resize: None,
            window: 0.0,
        }
    }
}

impl SidebarLayout {
    /// A sidebar as it was left: `width` (fitted to the allowed range) and collapsed or
    /// not, without a slide.
    pub fn restored(width: f32, collapsed: bool) -> Self {
        Self {
            width: if width.is_finite() {
                width.clamp(MIN_WIDTH, MAX_WIDTH)
            } else {
                DEFAULT_WIDTH
            },
            collapsed,
            reveal: if collapsed { 0.0 } else { 1.0 },
            ..Self::default()
        }
    }

    /// The chosen width, as remembered across launches.
    pub fn chosen_width(&self) -> f32 {
        self.width
    }

    /// Whether the user hid the sidebar.
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// The width it has when shown: the chosen width, fitted to the window.
    pub fn width(&self) -> f32 {
        let max = if self.window > 0.0 {
            (self.window / 2.0).clamp(MIN_WIDTH, MAX_WIDTH)
        } else {
            MAX_WIDTH
        };
        self.width.clamp(MIN_WIDTH, max)
    }

    /// How much of it is on screen now, 0 to 1, eased for the slide.
    pub fn shown(&self) -> f32 {
        let t = self.reveal.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Whether any of it is on screen.
    pub fn visible(&self) -> bool {
        self.reveal > 0.0
    }

    /// The slide position, 0 hidden to 1 shown (not eased).
    pub fn reveal(&self) -> f32 {
        self.reveal
    }

    /// Where the slide is heading.
    pub fn reveal_target(&self) -> f32 {
        if self.collapsed { 0.0 } else { 1.0 }
    }

    /// Moves the slide (the runner animates it).
    pub fn set_reveal(&mut self, reveal: f32) {
        self.reveal = reveal.clamp(0.0, 1.0);
    }

    /// Hides it if shown, shows it if hidden.
    pub fn toggle(&mut self) {
        self.collapsed = !self.collapsed;
    }

    /// The window's width it was last told (0 until then).
    pub fn window_width(&self) -> f32 {
        self.window
    }

    /// Tells it the window's width, which caps how wide it may be.
    pub fn set_window(&mut self, width: f32) {
        self.window = width.max(0.0);
    }

    /// Whether its edge is being dragged.
    pub fn resizing(&self) -> bool {
        self.resize.is_some()
    }

    /// Starts dragging its edge from a press at `x`.
    pub fn start_resize(&mut self, x: f32) {
        if !self.collapsed {
            self.resize = Some(Resize {
                offset: x - self.width(),
            });
        }
    }

    /// The pointer moved to `x` while dragging the edge. Narrower than [`SNAP_WIDTH`]
    /// collapses the sidebar (keeping its width for later); wider opens it again.
    pub fn drag_to(&mut self, x: f32) {
        let Some(resize) = self.resize else { return };
        let wanted = x - resize.offset;
        if wanted < SNAP_WIDTH {
            self.collapsed = true;
        } else {
            self.collapsed = false;
            self.width = wanted.clamp(MIN_WIDTH, MAX_WIDTH);
            self.width = self.width();
        }
    }

    /// The edge was let go.
    pub fn end_resize(&mut self) {
        self.resize = None;
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "exact widths are expected")]
mod tests {
    use super::*;

    #[test]
    fn dragging_the_edge_follows_the_pointer_within_limits() {
        let mut s = SidebarLayout::default();
        s.set_window(1400.0);
        // Grabbed 3 px right of the edge.
        s.start_resize(DEFAULT_WIDTH + 3.0);
        s.drag_to(343.0);
        assert_eq!(s.width(), 340.0);
        s.drag_to(5000.0);
        assert_eq!(s.width(), 560.0, "capped");
        s.set_window(900.0);
        assert_eq!(s.width(), 450.0, "never more than half the window");
        s.drag_to(180.0);
        assert_eq!(s.width(), MIN_WIDTH, "floored");
        assert!(!s.collapsed());
        s.end_resize();
        assert!(!s.resizing());
    }

    #[test]
    fn dragging_past_the_snap_point_collapses_and_back_reopens() {
        let mut s = SidebarLayout::default();
        s.start_resize(DEFAULT_WIDTH);
        s.drag_to(300.0);
        s.drag_to(100.0);
        assert!(s.collapsed());
        assert_eq!(s.chosen_width(), 300.0, "the width is kept for later");
        s.drag_to(250.0);
        assert!(!s.collapsed());
        assert_eq!(s.width(), 250.0);
    }

    #[test]
    fn a_collapsed_sidebar_has_no_edge_to_drag() {
        let mut s = SidebarLayout::default();
        s.toggle();
        s.start_resize(10.0);
        assert!(!s.resizing());
        assert_eq!(s.reveal_target(), 0.0);
        s.set_reveal(0.0);
        assert!(!s.visible());
    }

    #[test]
    fn restoring_fits_the_width_and_skips_the_slide() {
        let s = SidebarLayout::restored(9000.0, true);
        assert_eq!(s.chosen_width(), MAX_WIDTH);
        assert!(s.collapsed());
        assert_eq!(s.reveal(), 0.0);
        assert_eq!(
            SidebarLayout::restored(f32::NAN, false).width(),
            DEFAULT_WIDTH
        );
        assert_eq!(SidebarLayout::default().shown(), 1.0);
    }
}
