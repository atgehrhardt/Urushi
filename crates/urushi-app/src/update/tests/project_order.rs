//! Reordering projects in the sidebar: dragging a project onto another, the drop line,
//! and Move up / Move down (protocol 1.11).

use urushi_client::v1::{self, request::Body};
use urushi_ui::Point;

use super::{attached, bodies, workspace};
use crate::msg::{Effect, Msg};
use crate::organize::{Dragged, MenuTarget, PROJECT_ORDER_SINCE_MINOR, project_drop_index};
use crate::state::AppState;

/// The sample workspace plus projects A, B and C (ids `a`, `b`, `c`), on a daemon that
/// can reorder them.
fn with_projects() -> AppState {
    let mut s = attached();
    s.daemon_minor = PROJECT_ORDER_SINCE_MINOR;
    let mut w = workspace();
    for id in ["a", "b", "c"] {
        w.folders.push(v1::Folder {
            id: id.into(),
            name: id.to_uppercase(),
            ..Default::default()
        });
    }
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
}

fn moves(fx: &[Effect]) -> Vec<(String, u32)> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::MoveFolder(r) => Some((r.folder_id.clone(), r.index)),
            _ => None,
        })
        .collect()
}

fn drag(s: &mut AppState, project: &str) {
    s.update(Msg::Pointer(Point::new(10.0, 10.0)));
    s.update(Msg::DragProject(project.into()));
}

#[test]
fn a_dragged_project_takes_the_place_of_the_one_it_is_dropped_on() {
    let mut s = with_projects();
    drag(&mut s, "a");
    assert_eq!(
        s.dragging,
        Some((Dragged::Project("a".into()), Point::new(10.0, 10.0)))
    );
    let fx = s.update(Msg::DropProject("c".into()));
    assert_eq!(moves(&fx), [("a".to_owned(), 2)]);
    assert!(s.dragging.is_none());

    drag(&mut s, "c");
    let fx = s.update(Msg::DropProject("a".into()));
    assert_eq!(moves(&fx), [("c".to_owned(), 0)]);
}

#[test]
fn dropping_a_project_on_itself_or_cancelling_sends_nothing() {
    let mut s = with_projects();
    drag(&mut s, "b");
    assert!(s.update(Msg::DropProject("b".into())).is_empty());
    drag(&mut s, "b");
    s.update(Msg::DragCancel);
    assert!(s.dragging.is_none());
    // A thread drop while a project is dragged doesn't move the project.
    drag(&mut s, "b");
    let fx = s.update(Msg::DropThread(crate::organize::DropTarget::Ungrouped));
    assert!(bodies(&fx).is_empty());
}

#[test]
fn the_drop_line_follows_only_targets_that_would_move_the_project() {
    let mut s = with_projects();
    drag(&mut s, "b");
    s.update(Msg::DragOver(Some(Box::new(Msg::DropProject("c".into())))));
    assert_eq!(s.project_over.as_deref(), Some("c"));
    s.update(Msg::DragOver(Some(Box::new(Msg::DropProject("b".into())))));
    assert_eq!(s.project_over, None);
    s.update(Msg::DragOver(Some(Box::new(Msg::DropProject("a".into())))));
    assert_eq!(s.project_over.as_deref(), Some("a"));
    s.update(Msg::DragOver(None));
    assert_eq!(s.project_over, None);
    s.update(Msg::DragOver(Some(Box::new(Msg::DropProject("a".into())))));
    s.update(Msg::DragCancel);
    assert_eq!(s.project_over, None);
}

#[test]
fn move_up_and_down_send_the_neighbouring_index() {
    let mut s = with_projects();
    let menu_moves = |s: &mut AppState, project: &str| {
        s.update(Msg::OpenMenu(MenuTarget::Project(project.into())));
        let frame = render(s);
        let mut found: Vec<(String, u32)> = frame
            .hits
            .iter()
            .filter_map(|h| match &h.on_click {
                Some(Msg::MoveProject { project, index }) => Some((project.clone(), *index)),
                _ => None,
            })
            .collect();
        found.sort();
        s.update(Msg::CloseMenu);
        found
    };
    // The first can only go down, the last only up, the middle both ways.
    assert_eq!(menu_moves(&mut s, "a"), [("a".to_owned(), 1)]);
    assert_eq!(
        menu_moves(&mut s, "b"),
        [("b".to_owned(), 0), ("b".to_owned(), 2)]
    );
    assert_eq!(menu_moves(&mut s, "c"), [("c".to_owned(), 1)]);
    s.daemon_minor = PROJECT_ORDER_SINCE_MINOR - 1;
    assert!(menu_moves(&mut s, "b").is_empty());
    s.daemon_minor = PROJECT_ORDER_SINCE_MINOR;

    s.update(Msg::OpenMenu(MenuTarget::Project("b".into())));
    let fx = s.update(Msg::MoveProject {
        project: "b".into(),
        index: 0,
    });
    assert_eq!(moves(&fx), [("b".to_owned(), 0)]);
    assert!(s.menu.is_none());
}

#[test]
fn an_older_daemon_cannot_drag_projects() {
    let mut s = with_projects();
    s.daemon_minor = PROJECT_ORDER_SINCE_MINOR - 1;
    drag(&mut s, "a");
    assert!(s.dragging.is_none());
}

#[test]
fn drop_indexes_count_without_the_dragged_project() {
    let s = with_projects();
    let p = s.workspace.projects();
    assert_eq!(project_drop_index(p, "a", "b"), Some(1));
    assert_eq!(project_drop_index(p, "a", "c"), Some(2));
    assert_eq!(project_drop_index(p, "c", "b"), Some(1));
    assert_eq!(project_drop_index(p, "c", "c"), None);
    assert_eq!(project_drop_index(p, "x", "a"), None);
}

fn render(s: &AppState) -> urushi_ui::Frame<Msg> {
    let mut text = urushi_ui::TextSystem::bundled_only();
    let root = crate::view::view(
        s,
        &urushi_ui::Theme::dark(),
        &std::collections::HashMap::new(),
    );
    urushi_ui::build(
        &root,
        urushi_ui::Rect::new(0.0, 0.0, 1200.0, 800.0),
        &mut text,
        None,
    )
}

#[test]
fn project_headers_drag_and_each_project_accepts_a_dragged_project() {
    let mut s = with_projects();
    let frame = render(&s);
    let draggable = |f: &urushi_ui::Frame<Msg>| {
        f.hits
            .iter()
            .filter(|h| matches!(&h.on_drag, Some(Msg::DragProject(_))))
            .count()
    };
    assert_eq!(draggable(&frame), 3);

    // Only projects take a dragged project; "No project" doesn't.
    drag(&mut s, "a");
    let frame = render(&s);
    let targets: Vec<&Msg> = frame
        .hits
        .iter()
        .filter_map(|h| h.on_drop.as_ref())
        .collect();
    assert_eq!(targets.len(), 3);
    assert!(targets.iter().all(|m| matches!(m, Msg::DropProject(_))));
    s.update(Msg::DragCancel);

    // An older daemon can't reorder, so nothing offers it.
    s.daemon_minor = PROJECT_ORDER_SINCE_MINOR - 1;
    assert_eq!(draggable(&render(&s)), 0);
}
