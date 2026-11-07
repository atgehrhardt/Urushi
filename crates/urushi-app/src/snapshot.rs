//! Builds the scene of a fresh app state, for offscreen snapshots (see
//! `examples/snapshot.rs`) and layout checks at different window sizes.

use std::collections::HashMap;

use urushi_ui::{CellMetrics, Rect, Scene, TextSystem, Theme, build};

use crate::config::ConnectTarget;
use crate::state::AppState;

/// The scene of a freshly started app connecting to `target`, at `width`×`height` logical pixels.
pub fn scene(
    target: ConnectTarget,
    theme: &Theme,
    text: &mut TextSystem,
    width: f32,
    height: f32,
) -> Scene {
    let cell: CellMetrics = text.cell_metrics(theme.terminal_font_size);
    let mut state = AppState::new(target, cell);
    state.compact = width < 700.0;
    let root = crate::view::view(&state, theme, &HashMap::new());
    build(&root, Rect::new(0.0, 0.0, width, height), text, None).scene
}
