//! `view(state) → Node<Msg>`: the whole UI as a function of state.

mod dialogs;
mod main_area;
mod sidebar;

use std::collections::HashMap;

use urushi_ui::{Key, Length, Node, Theme, key};

use crate::msg::Msg;
use crate::state::AppState;

/// Width of the sidebar.
pub(crate) const SIDEBAR_WIDTH: f32 = 264.0;

/// Key of the terminal area. The runner looks it up in the frame to learn its rectangle.
pub(crate) fn area_key() -> Key {
    key(&["terminal-area"])
}

/// Key of the sidebar's scrolling list.
pub(crate) fn sidebar_scroll_key() -> Key {
    key(&["sidebar-scroll"])
}

/// The whole window.
pub(crate) fn view(state: &AppState, theme: &Theme, scroll: &HashMap<Key, f32>) -> Node<Msg> {
    let sidebar_scroll = scroll.get(&sidebar_scroll_key()).copied().unwrap_or(0.0);
    let mut app = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .align(urushi_ui::Align::Stretch)
        .background(theme.background);
    if state.compact {
        // Phones: the thread list or the terminals, never both.
        app = if state.sidebar_open
            || state.current_tab().is_none() && state.selected_thread.is_none()
        {
            app.child(sidebar::view(
                state,
                theme,
                sidebar_scroll,
                Length::Fill(1.0),
            ))
        } else {
            app.child(main_area::view(state, theme))
        };
    } else {
        app = app
            .child(sidebar::view(
                state,
                theme,
                sidebar_scroll,
                Length::Fixed(SIDEBAR_WIDTH),
            ))
            .child(urushi_ui::widgets::divider(theme, true))
            .child(main_area::view(state, theme));
    }
    let mut root = Node::stack()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .child(app);
    if let Some(dialog) = dialogs::view(state, theme) {
        root = root.child(dialog);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConnectTarget;
    use urushi_ui::{CellMetrics, Point, Rect, TextSystem, build};

    #[test]
    fn empty_workspace_offers_to_add_a_project() {
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        let state = AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        );
        let frame = build(
            &view(&state, &theme, &HashMap::new()),
            Rect::new(0.0, 0.0, 1200.0, 800.0),
            &mut text,
            None::<Point>,
        );
        let dump = frame.scene.dump();
        assert!(dump.contains("\"Urushi\""), "{dump}");
        assert!(
            frame
                .hits
                .iter()
                .any(|h| matches!(h.on_click, Some(Msg::OpenAddProject)))
        );
    }
}
