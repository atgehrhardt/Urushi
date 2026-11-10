//! `view(state) → Node<Msg>`: the whole UI as a function of state.

mod appearance;
mod attention;
mod board;
mod daemon;
mod dialogs;
mod jira;
mod labels;
mod launcher;
mod main_area;
mod menu;
mod new_thread;
mod project_settings;
mod projects;
mod setup;
mod sidebar;
mod status_bar;
mod task_in_thread;
mod tasks;
mod thread_card;

use std::collections::HashMap;

use urushi_ui::widgets::panel;
use urushi_ui::{CursorIcon, Insets, Key, Length, Node, Quad, Rect, Role, Theme, key};

use crate::msg::{Msg, SidebarMsg};
use crate::state::AppState;

/// Space around the glass panels, between them and the window's edge.
pub(crate) const PANEL_MARGIN: f32 = 8.0;
/// The gap between the sidebar and the main panel. Dragging it resizes the sidebar.
pub(crate) const EDGE: f32 = 8.0;

/// Key of the terminal area. The runner looks it up in the frame to learn its rectangle.
pub(crate) fn area_key() -> Key {
    key(&["terminal-area"])
}

/// Key of the sidebar's scrolling list.
pub(crate) fn sidebar_scroll_key() -> Key {
    key(&["sidebar-scroll"])
}

/// The whole window: the glowing backdrop, glass panels over it, then dialogs, menus
/// and drag ghosts.
pub(crate) fn view(state: &AppState, theme: &Theme, scroll: &HashMap<Key, f32>) -> Node<Msg> {
    let sidebar_scroll = scroll.get(&sidebar_scroll_key()).copied().unwrap_or(0.0);
    let mut app = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .align(urushi_ui::Align::Stretch);
    if state.compact {
        // Phones: the thread list or the terminals, never both, edge to edge.
        let page = if state.sidebar_open
            || state.current_tab().is_none() && state.selected_thread.is_none()
        {
            sidebar::view(state, theme, sidebar_scroll, Length::Fill(1.0))
        } else {
            main_area::view(state, theme)
        };
        app = app.child(page.background(theme.glass).gradient(theme.glass_low));
        // The task board covers the whole screen.
        app = with_board(state, theme, scroll, app);
    } else {
        app = app.padding(Insets::all(PANEL_MARGIN));
        if state.sidebar.visible() {
            app = app.child(sidebar_slot(state, theme, sidebar_scroll));
        }
        // The task board slides out from under the sidebar, over the terminals.
        app = app.child(with_board(
            state,
            theme,
            scroll,
            panel(theme, main_area::view(state, theme)),
        ));
    }
    let mut root = Node::stack()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .child(backdrop(theme))
        .child(app);
    if let Some(dialog) = dialogs::view(state, theme, scroll) {
        root = root.child(dialog);
    }
    root.children(menu::overlay(state, theme, scroll))
        .children(menu::drag_ghost(state, theme))
        .children(board::drag_ghost(state, theme))
}

/// The soft colour glows behind everything, which the glass panels tint.
fn backdrop(theme: &Theme) -> Node<Msg> {
    let glows = theme.glows.clone();
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .paint(move |rect, scene, _text| {
            let side = rect.width.max(rect.height);
            for glow in &glows {
                // Fully feathered: solid at the centre, gone at `radius`.
                let r = glow.radius * side / 2.0;
                let (x, y) = (
                    rect.x + glow.center.0 * rect.width,
                    rect.y + glow.center.1 * rect.height,
                );
                scene.quad(Quad {
                    rect: Rect::new(x - r, y - r, 2.0 * r, 2.0 * r),
                    color: glow.color,
                    radius: r,
                    softness: 2.0 * r,
                    ..Quad::default()
                });
            }
        })
}

/// The sidebar panel and the edge that resizes it, cut to how far it has slid out.
fn sidebar_slot(state: &AppState, theme: &Theme, scroll: f32) -> Node<Msg> {
    let width = state.sidebar.width();
    let shown = state.sidebar.shown();
    let edge_color = if state.sidebar.resizing() {
        theme.accent.with_alpha(0.45)
    } else {
        urushi_ui::Color::TRANSPARENT
    };
    let mut edge = Node::new()
        .width(Length::Fixed(EDGE))
        .height(Length::Fill(1.0))
        .padding(Insets::symmetric(12.0, 3.0))
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .radius(1.0)
                .background(edge_color),
        )
        .hover_background(theme.accent.with_alpha(0.18))
        .radius(EDGE / 2.0)
        .key(key(&["sidebar-edge"]))
        .on_click(Msg::Sidebar(SidebarMsg::EdgePressed))
        .on_drag(Msg::Sidebar(SidebarMsg::StartResize))
        .role(Role::Button, "Resize sidebar");
    edge.cursor = CursorIcon::ResizeColumn;
    Node::row()
        .width(Length::Fixed((width + EDGE) * shown))
        .height(Length::Fill(1.0))
        .clip()
        .child(
            Node::row()
                .width(Length::Fixed(width + EDGE))
                .height(Length::Fill(1.0))
                .align(urushi_ui::Align::Stretch)
                .slide_x(1.0 - shown)
                .child(panel(
                    theme,
                    sidebar::view(state, theme, scroll, Length::Fixed(width)),
                ))
                .child(edge),
        )
}

/// The button that hides or shows the sidebar.
pub(super) fn sidebar_toggle(state: &AppState, theme: &Theme) -> Node<Msg> {
    let label = if state.sidebar.collapsed() {
        "Show sidebar"
    } else {
        "Hide sidebar"
    };
    urushi_ui::widgets::sidebar_button(theme, label, Msg::Sidebar(SidebarMsg::Toggle))
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
        let base_switch = |frame: &urushi_ui::Frame<Msg>| {
            frame.hits.iter().any(|h| {
                matches!(
                    h.on_click,
                    Some(Msg::Toggle(
                        crate::state::SettingToggle::UpdateBaseFromRemote
                    ))
                )
            })
        };
        assert!(!base_switch(&frame), "an older daemon doesn't update bases");

        state.daemon_minor = urushi_proto::PROTOCOL_MINOR;
        let frame = render(&state, &mut text);
        assert!(!frame.scene.dump().contains("older than this app"));
        assert!(base_switch(&frame));
        assert!(
            frame
                .scene
                .dump()
                .contains("Update the base branch from its remote")
        );
        assert!(frame.hits.iter().any(|h| matches!(
            h.on_click,
            Some(Msg::Toggle(crate::state::SettingToggle::SkipPermissions(_)))
        )));
    }

    #[test]
    fn the_sidebar_resizes_from_its_edge_and_hides_behind_a_button() {
        use crate::msg::SidebarMsg;
        let mut text = TextSystem::bundled_only();
        let mut state = organized();
        let frame = render(&state, &mut text);
        let edge = frame
            .hits
            .iter()
            .find(|h| matches!(h.on_drag, Some(Msg::Sidebar(SidebarMsg::StartResize))))
            .expect("the edge can be dragged");
        assert_eq!(edge.cursor, urushi_ui::CursorIcon::ResizeColumn);
        assert!(
            (edge.rect.x - (PANEL_MARGIN + crate::sidebar::DEFAULT_WIDTH)).abs() < 0.5,
            "right of the sidebar: {:?}",
            edge.rect
        );
        let hide = frame
            .access
            .iter()
            .find(|a| a.label == "Hide sidebar")
            .expect("a hide button");
        assert!(matches!(
            frame.click(hide.rect.center()),
            Some(Msg::Sidebar(SidebarMsg::Toggle))
        ));

        // A wider sidebar moves the edge with it.
        state.update(Msg::Pointer(edge.rect.center()));
        state.update(Msg::Sidebar(SidebarMsg::StartResize));
        state.update(Msg::DragMove(Point::new(
            edge.rect.center().x + 80.0,
            300.0,
        )));
        state.update(Msg::DragCancel);
        let moved = render(&state, &mut text)
            .hits
            .iter()
            .find(|h| matches!(h.on_drag, Some(Msg::Sidebar(SidebarMsg::StartResize))))
            .map(|h| h.rect.x);
        assert!(moved.is_some_and(|x| (x - edge.rect.x - 80.0).abs() < 0.5));

        // Hidden: the list is gone, and the tab bar has the button to bring it back.
        state.update(Msg::SelectThread("a".into()));
        state.update(Msg::Sidebar(SidebarMsg::Toggle));
        state.update(Msg::Sidebar(SidebarMsg::Reveal(0.0)));
        let frame = render(&state, &mut text);
        assert!(!frame.scene.dump().contains("Thread b"), "sidebar hidden");
        assert!(frame.access.iter().any(|a| a.label == "Show sidebar"));
        assert!(
            !frame
                .hits
                .iter()
                .any(|h| matches!(h.on_drag, Some(Msg::Sidebar(SidebarMsg::StartResize)))),
            "no edge while hidden"
        );
    }

    /// Collapses the sidebar (the slide finished) and clicks "Show sidebar", which
    /// must exist and bring it back whatever the main area shows.
    fn collapse_and_show(state: &mut AppState, text: &mut TextSystem, case: &str) {
        use crate::msg::SidebarMsg;
        state.update(Msg::Sidebar(SidebarMsg::Toggle));
        state.update(Msg::Sidebar(SidebarMsg::Reveal(0.0)));
        let frame = render(state, text);
        // A button the user can see and click: one covered by the board doesn't count.
        let reachable = frame
            .access
            .iter()
            .filter(|a| a.label == "Show sidebar")
            .any(|a| {
                matches!(
                    frame.click(a.rect.center()),
                    Some(Msg::Sidebar(SidebarMsg::Toggle))
                )
            });
        assert!(reachable, "{case}: no way to show the sidebar");
        state.update(Msg::Sidebar(SidebarMsg::Toggle));
        state.update(Msg::Sidebar(SidebarMsg::Reveal(1.0)));
        assert!(
            render(state, text).scene.dump().contains("Thread b"),
            "{case}: shown again"
        );
    }

    #[test]
    fn a_collapsed_sidebar_can_always_be_shown_again() {
        let mut text = TextSystem::bundled_only();
        // No thread chosen yet (as after a restart with the sidebar collapsed).
        let mut state = organized();
        collapse_and_show(&mut state, &mut text, "no thread");
        // A thread with no tabs (the launcher).
        state.update(Msg::SelectThread("a".into()));
        collapse_and_show(&mut state, &mut text, "launcher");
        // The task board out over the terminals.
        state.update(Msg::ToggleBoard);
        state.update(Msg::BoardReveal(1.0));
        collapse_and_show(&mut state, &mut text, "task board");
        state.update(Msg::ToggleBoard);
        state.update(Msg::BoardReveal(0.0));
        // Nothing to show at all: no repos yet.
        let mut empty = organized();
        empty.update(Msg::Daemon(
            urushi_client::v1::event::Body::WorkspaceChanged(
                urushi_client::v1::Workspace::default(),
            ),
        ));
        empty.update(Msg::Sidebar(crate::msg::SidebarMsg::Toggle));
        empty.update(Msg::Sidebar(crate::msg::SidebarMsg::Reveal(0.0)));
        let frame = render(&empty, &mut text);
        assert!(
            frame
                .access
                .iter()
                .filter(|a| a.label == "Show sidebar")
                .any(|a| matches!(
                    frame.click(a.rect.center()),
                    Some(Msg::Sidebar(crate::msg::SidebarMsg::Toggle))
                )),
            "empty workspace: no way to show the sidebar"
        );
    }
}
