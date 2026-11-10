//! Reordering threads in the sidebar (protocol 1.21): dropping a thread onto another takes
//! its place and joins its group, the drop line, Move up / Move down, and older daemons.

use urushi_client::v1::{self, request::Body};
use urushi_ui::Point;

use super::{attached, bodies, workspace};
use crate::msg::{Effect, Msg};
use crate::organize::{Dragged, MenuTarget, THREAD_ORDER_SINCE_MINOR};
use crate::state::AppState;

/// The sample workspace (its ungrouped thread "Fix bug" first) plus projects `p` and `q`
/// and threads, in this order: `t1`, `t2` (in `p`), `t3` (in `p`, status Review), `t4`
/// (in `p`, pinned), `t5` (in `q`), `t6` (in `p`), and `p`'s parent-branch thread `base`.
fn with_threads() -> AppState {
    let mut s = attached();
    s.daemon_minor = THREAD_ORDER_SINCE_MINOR;
    let mut w = workspace();
    for id in ["p", "q"] {
        w.folders.push(v1::Folder {
            id: id.into(),
            name: id.to_uppercase(),
            ..Default::default()
        });
    }
    let repo = w.projects[0].id.clone();
    let thread = |id: &str, folder: &str, status: &str, pinned: bool| v1::Thread {
        id: id.into(),
        project_id: repo.clone(),
        title: id.to_uppercase(),
        folder_id: folder.into(),
        status: status.into(),
        pinned,
        state: v1::ThreadState::Ready.into(),
        ..Default::default()
    };
    w.threads.extend([
        thread("t1", "p", "", false),
        thread("t2", "p", "", false),
        thread("t3", "p", "Review", false),
        thread("t4", "p", "", true),
        thread("t5", "q", "", false),
        thread("t6", "p", "", false),
        v1::Thread {
            project_base: true,
            ..thread("base", "p", "", false)
        },
    ]);
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
}

fn fix_bug(s: &AppState) -> String {
    s.workspace.raw().threads[0].id.clone()
}

fn drag(s: &mut AppState, thread: &str) {
    s.update(Msg::Pointer(Point::new(10.0, 10.0)));
    s.update(Msg::DragThread(thread.into()));
}

fn drop_on(s: &mut AppState, thread: &str, target: &str) -> Vec<Effect> {
    drag(s, thread);
    s.update(Msg::DropOnThread(target.into()))
}

fn moves(fx: &[Effect]) -> Vec<(String, u32)> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::MoveThread(r) => Some((r.thread_id.clone(), r.index)),
            _ => None,
        })
        .collect()
}

fn updates(fx: &[Effect]) -> Vec<v1::UpdateThreadRequest> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::UpdateThread(r) => Some(r.clone()),
            _ => None,
        })
        .collect()
}

fn update(thread: &str, f: impl FnOnce(&mut v1::UpdateThreadRequest)) -> v1::UpdateThreadRequest {
    let mut r = v1::UpdateThreadRequest {
        thread_id: thread.into(),
        ..Default::default()
    };
    f(&mut r);
    r
}

#[test]
fn a_thread_dropped_in_its_own_group_only_takes_the_targets_place() {
    let mut s = with_threads();
    // Down the list it lands below the target, up the list above it.
    let fx = drop_on(&mut s, "t1", "t6");
    assert_eq!(moves(&fx), [("t1".to_owned(), 6)]);
    assert!(updates(&fx).is_empty());
    assert!(s.dragging.is_none());
    let fx = drop_on(&mut s, "t6", "t2");
    assert_eq!(moves(&fx), [("t6".to_owned(), 2)]);
    assert!(updates(&fx).is_empty());
}

#[test]
fn a_thread_dropped_in_another_group_joins_it() {
    let mut s = with_threads();
    // Onto a status section's thread: takes the status.
    let fx = drop_on(&mut s, "t1", "t3");
    assert_eq!(
        updates(&fx),
        [update("t1", |r| r.status = Some("Review".into()))]
    );
    assert_eq!(moves(&fx), [("t1".to_owned(), 3)]);
    // Onto a pinned thread: pinned, keeping its status.
    let fx = drop_on(&mut s, "t3", "t4");
    assert_eq!(updates(&fx), [update("t3", |r| r.pinned = Some(true))]);
    assert_eq!(moves(&fx), [("t3".to_owned(), 4)]);
    // A pinned thread onto an unpinned one: unpinned.
    let fx = drop_on(&mut s, "t4", "t1");
    assert_eq!(updates(&fx), [update("t4", |r| r.pinned = Some(false))]);
    assert_eq!(moves(&fx), [("t4".to_owned(), 1)]);
    // Onto a thread in another project: filed there.
    let fx = drop_on(&mut s, "t1", "t5");
    assert_eq!(
        updates(&fx),
        [update("t1", |r| r.folder_id = Some("q".into()))]
    );
    assert_eq!(moves(&fx), [("t1".to_owned(), 5)]);
    // Onto an ungrouped thread: ungrouped.
    let first = fix_bug(&s);
    let fx = drop_on(&mut s, "t5", &first);
    assert_eq!(
        updates(&fx),
        [update("t5", |r| r.folder_id = Some(String::new()))]
    );
    assert_eq!(moves(&fx), [("t5".to_owned(), 0)]);
}

#[test]
fn dropping_on_itself_or_a_parent_branch_thread_sends_nothing() {
    let mut s = with_threads();
    assert!(drop_on(&mut s, "t2", "t2").is_empty());
    assert!(drop_on(&mut s, "t2", "base").is_empty());
    // Nothing happens without a dragged thread.
    assert!(s.update(Msg::DropOnThread("t6".into())).is_empty());
}

#[test]
fn the_drop_line_follows_threads_that_would_take_the_dragged_one() {
    let mut s = with_threads();
    drag(&mut s, "t2");
    let over = |s: &mut AppState, target: &str| {
        s.update(Msg::DragOver(Some(Box::new(Msg::DropOnThread(
            target.into(),
        )))));
        s.thread_over.clone()
    };
    assert_eq!(over(&mut s, "t6").as_deref(), Some("t6"));
    assert_eq!(over(&mut s, "t2"), None);
    assert_eq!(over(&mut s, "base"), None);
    assert_eq!(over(&mut s, "t1").as_deref(), Some("t1"));
    // Over a project rather than a thread: no line.
    s.update(Msg::DragOver(Some(Box::new(Msg::DropThread(
        crate::organize::DropTarget::Project("q".into()),
    )))));
    assert_eq!(s.thread_over, None);
    over(&mut s, "t1");
    s.update(Msg::DragCancel);
    assert_eq!(s.thread_over, None);
    assert!(s.dragging.is_none());
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

fn thread_targets(frame: &urushi_ui::Frame<Msg>) -> Vec<String> {
    frame
        .hits
        .iter()
        .filter_map(|h| match &h.on_drop {
            Some(Msg::DropOnThread(t)) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn every_other_thread_but_the_parent_branch_one_takes_a_dragged_thread() {
    let mut s = with_threads();
    assert!(thread_targets(&render(&s)).is_empty());
    drag(&mut s, "t2");
    let mut targets = thread_targets(&render(&s));
    targets.sort();
    let mut expected = vec![fix_bug(&s)];
    expected.extend(["t1", "t3", "t4", "t5", "t6"].map(String::from));
    expected.sort();
    assert_eq!(targets, expected);
    assert!(
        render(&s)
            .hits
            .iter()
            .any(|h| matches!(&h.on_drop, Some(Msg::DropThread(_)))),
        "projects still take a dragged thread"
    );

    // Where the line shows, the line takes the drop too.
    s.update(Msg::DragOver(Some(Box::new(Msg::DropOnThread(
        "t6".into(),
    )))));
    let targets = thread_targets(&render(&s));
    assert_eq!(targets.iter().filter(|t| *t == "t6").count(), 2);
}

#[test]
fn move_up_and_down_stay_within_the_threads_group() {
    let mut s = with_threads();
    let menu_moves = |s: &mut AppState, thread: &str| {
        s.update(Msg::OpenMenu(MenuTarget::Thread(thread.into())));
        let mut found: Vec<(String, u32)> = render(s)
            .hits
            .iter()
            .filter_map(|h| match &h.on_click {
                Some(Msg::ReorderThread { thread, index }) => Some((thread.clone(), *index)),
                _ => None,
            })
            .collect();
        found.sort();
        s.update(Msg::CloseMenu);
        found
    };
    // P's unpinned threads without a status are t1, t2 and t6.
    assert_eq!(menu_moves(&mut s, "t1"), [("t1".to_owned(), 2)]);
    assert_eq!(
        menu_moves(&mut s, "t2"),
        [("t2".to_owned(), 1), ("t2".to_owned(), 6)]
    );
    assert_eq!(menu_moves(&mut s, "t6"), [("t6".to_owned(), 2)]);
    // Alone in their groups.
    assert!(menu_moves(&mut s, "t3").is_empty());
    assert!(menu_moves(&mut s, "t4").is_empty());
    assert!(menu_moves(&mut s, "t5").is_empty());

    s.update(Msg::OpenMenu(MenuTarget::Thread("t2".into())));
    let fx = s.update(Msg::ReorderThread {
        thread: "t2".into(),
        index: 1,
    });
    assert_eq!(moves(&fx), [("t2".to_owned(), 1)]);
    assert!(s.menu.is_none());
}

#[test]
fn an_older_daemon_only_moves_threads_between_projects() {
    let mut s = with_threads();
    s.daemon_minor = THREAD_ORDER_SINCE_MINOR - 1;
    drag(&mut s, "t2");
    assert_eq!(
        s.dragging,
        Some((Dragged::Thread("t2".into()), Point::new(10.0, 10.0)))
    );
    assert!(thread_targets(&render(&s)).is_empty());
    s.update(Msg::DragCancel);

    s.update(Msg::OpenMenu(MenuTarget::Thread("t2".into())));
    assert!(
        !render(&s)
            .hits
            .iter()
            .any(|h| matches!(&h.on_click, Some(Msg::ReorderThread { .. })))
    );
    s.update(Msg::CloseMenu);

    // A drop that arrives anyway joins the group but sends no `MoveThread`.
    let fx = drop_on(&mut s, "t2", "t5");
    assert!(moves(&fx).is_empty());
    assert_eq!(
        updates(&fx),
        [update("t2", |r| r.folder_id = Some("q".into()))]
    );
}
