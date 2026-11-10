//! Messages about the desktop sidebar: hiding and showing it, dragging its edge, and
//! its slide (see `crate::sidebar`).

/// Something done to the sidebar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SidebarMsg {
    /// Hide it if shown, show it if hidden (the button, or Cmd+\).
    Toggle,
    /// The edge was pressed. Nothing happens unless it becomes a drag; the press only
    /// makes the edge something the runner can start a drag on.
    EdgePressed,
    /// Dragging the edge started.
    StartResize,
    /// The slide advanced (0 = hidden, 1 = shown).
    Reveal(f32),
    /// The window's width, in logical pixels, which caps the sidebar's.
    WindowWidth(f32),
}
