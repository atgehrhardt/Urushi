//! `view(state) → Node<Msg>`: the whole UI as a function of state.

mod attention;
mod board;
mod daemon;
mod dialogs;
mod labels;
mod launcher;
mod main_area;
mod menu;
mod new_thread;
mod projects;
mod setup;
mod sidebar;
mod status_bar;
mod task_in_thread;
mod tasks;

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
        // The task board covers the whole screen.
        app = with_board(state, theme, scroll, app);
    } else {
        app = app
            .child(sidebar::view(
                state,
                theme,
                sidebar_scroll,
                Length::Fixed(SIDEBAR_WIDTH),
            ))
            .child(urushi_ui::widgets::divider(theme, true))
            // The task board slides out from under the sidebar, over the terminals.
            .child(with_board(
                state,
                theme,
                scroll,
                main_area::view(state, theme),
            ));
    }
    let mut root = Node::stack()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .child(app);
    if let Some(dialog) = dialogs::view(state, theme, scroll) {
        root = root.child(dialog);
    }
    root.children(menu::overlay(state, theme))
        .children(menu::drag_ghost(state, theme))
        .children(board::drag_ghost(state, theme))
}

/// `content`, with the task-board panel over it while the panel is (partly) out.
fn with_board(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    content: Node<Msg>,
) -> Node<Msg> {
    Node::stack()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .clip()
        .child(content)
        .children(
            state
                .board
                .visible()
                .then(|| board::panel(state, theme, scroll)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConnectTarget;
    use urushi_ui::{CellMetrics, Point, Rect, TextSystem, build};

    #[test]
    fn empty_workspace_offers_to_add_a_repo() {
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        let mut state = AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        );
        state.update(Msg::Connected {
            welcome: urushi_client::v1::Welcome::default(),
            workspace: urushi_client::v1::Workspace::default(),
        });
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

    #[test]
    fn a_thread_without_tabs_keeps_the_new_tab_button_and_shows_the_launcher() {
        use urushi_client::v1;
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        let mut state = AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        );
        state.update(Msg::Connected {
            welcome: v1::Welcome::default(),
            workspace: v1::Workspace {
                projects: vec![v1::Project {
                    id: "p".into(),
                    ..Default::default()
                }],
                threads: vec![v1::Thread {
                    id: "t".into(),
                    project_id: "p".into(),
                    title: "Primary".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        });
        // Agents are reported by daemons speaking protocol 1.1 or later.
        state.daemon_minor = 1;
        state.agents = vec![urushi_core::Agent::Claude];
        let frame = build(
            &view(&state, &theme, &HashMap::new()),
            Rect::new(0.0, 0.0, 1200.0, 800.0),
            &mut text,
            None::<Point>,
        );
        let clicks: Vec<_> = frame
            .hits
            .iter()
            .filter_map(|h| h.on_click.clone())
            .collect();
        assert!(
            clicks.iter().any(|m| matches!(m, Msg::NewTab)),
            "the + button"
        );
        for launch in [
            crate::state::Launch::Terminal,
            crate::state::Launch::Agent(urushi_core::Agent::Claude),
        ] {
            assert!(
                clicks
                    .iter()
                    .any(|m| matches!(m, Msg::Launch(l) if *l == launch)),
                "{launch:?} offered"
            );
        }
        let dump = frame.scene.dump();
        assert!(dump.contains("Start working in “Primary”"), "{dump}");
        assert!(!dump.contains("Codex"), "only installed agents are offered");
    }

    /// A workspace with one repo, a project "Billing" holding a pinned thread and one
    /// "In review", and an ungrouped thread.
    fn organized() -> AppState {
        use urushi_client::v1;
        let mut state = AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        );
        let thread = |id: &str, folder: &str, status: &str, pinned: bool| v1::Thread {
            id: id.into(),
            project_id: "r".into(),
            title: format!("Thread {id}"),
            primary_branch: format!("urushi/{id}"),
            folder_id: folder.into(),
            status: status.into(),
            pinned,
            state: v1::ThreadState::Ready.into(),
            ..Default::default()
        };
        state.update(Msg::Connected {
            welcome: v1::Welcome::default(),
            workspace: v1::Workspace {
                projects: vec![v1::Project {
                    id: "r".into(),
                    name: "api-repo".into(),
                    ..Default::default()
                }],
                folders: vec![v1::Folder {
                    id: "f".into(),
                    name: "Billing".into(),
                    ..Default::default()
                }],
                threads: vec![
                    thread("a", "f", "In review", false),
                    thread("b", "f", "", true),
                    thread("c", "", "", false),
                ],
                ..Default::default()
            },
        });
        state
    }

    fn render(state: &AppState, text: &mut TextSystem) -> urushi_ui::Frame<Msg> {
        build(
            &view(state, &Theme::dark(), &HashMap::new()),
            Rect::new(0.0, 0.0, 1200.0, 800.0),
            text,
            None::<Point>,
        )
    }

    #[test]
    fn the_sidebar_groups_threads_by_project_pin_and_status() {
        let mut text = TextSystem::bundled_only();
        let mut state = organized();
        let frame = render(&state, &mut text);
        let dump = frame.scene.dump();
        let at = |needle: &str| {
            dump.find(needle)
                .unwrap_or_else(|| panic!("{needle} missing: {dump}"))
        };
        // Project, pinned thread, status section with its thread, then ungrouped below.
        assert!(at("\"Billing\"") < at("Thread b"));
        assert!(at("Thread b") < at("IN REVIEW"));
        assert!(at("IN REVIEW") < at("Thread a"));
        assert!(at("Thread a") < at("\"No project\""));
        assert!(at("\"No project\"") < at("Thread c"));
        assert!(
            dump.contains("api-repo · urushi/c"),
            "cards show their repo"
        );

        // Collapsing the status section hides its thread.
        state.update(Msg::ToggleSection(crate::organize::section_key(
            Some("f"),
            Some("In review"),
        )));
        assert!(
            !render(&state, &mut text)
                .scene
                .dump()
                .contains("api-repo · urushi/a")
        );
    }

    #[test]
    fn threads_open_menus_and_drag_onto_projects() {
        let mut text = TextSystem::bundled_only();
        let mut state = organized();
        let frame = render(&state, &mut text);
        assert!(
            frame
                .hits
                .iter()
                .any(|h| matches!(&h.on_secondary_click, Some(Msg::OpenMenu(_))))
        );
        assert!(
            frame
                .hits
                .iter()
                .any(|h| matches!(&h.on_drag, Some(Msg::DragThread(_))))
        );
        assert!(
            frame.hits.iter().all(|h| h.on_drop.is_none()),
            "no drop targets until a drag"
        );

        // While dragging, projects and "No project" accept drops.
        state.update(Msg::DragThread("c".into()));
        let frame = render(&state, &mut text);
        assert_eq!(frame.hits.iter().filter(|h| h.on_drop.is_some()).count(), 2);
        state.update(Msg::DragCancel);

        // The menu draws on top, where the press was.
        state.update(Msg::Pointer(Point::new(100.0, 100.0)));
        state.update(Msg::OpenMenu(crate::organize::MenuTarget::Thread(
            "a".into(),
        )));
        let frame = render(&state, &mut text);
        assert!(
            matches!(
                frame.click(Point::new(110.0, 110.0)),
                Some(Msg::OpenPrompt(_))
            ),
            "first item: Rename"
        );
        assert!(
            matches!(frame.click(Point::new(900.0, 700.0)), Some(Msg::CloseMenu)),
            "clicking away closes"
        );
    }

    #[test]
    fn settings_explain_an_outdated_daemon_instead_of_offering_dead_switches() {
        let mut text = TextSystem::bundled_only();
        let mut state = organized(); // protocol 1.0, like a daemon started before the update
        state.update(Msg::OpenSettings);
        let frame = render(&state, &mut text);
        assert!(frame.scene.dump().contains("older than this app"));
        assert!(
            !frame.hits.iter().any(|h| matches!(
                h.on_click,
                Some(Msg::Toggle(crate::state::SettingToggle::SkipPermissions(_)))
            )),
            "agent switches are inert"
        );

        assert!(
            frame
                .hits
                .iter()
                .any(|h| matches!(h.on_click, Some(Msg::OpenRestartDaemon))),
            "and offer the restart"
        );

        state.daemon_minor = urushi_proto::PROTOCOL_MINOR;
        let frame = render(&state, &mut text);
        assert!(!frame.scene.dump().contains("older than this app"));
        assert!(frame.hits.iter().any(|h| matches!(
            h.on_click,
            Some(Msg::Toggle(crate::state::SettingToggle::SkipPermissions(_)))
        )));
    }
}
