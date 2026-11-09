//! The sidebar's organizing flows: context menu, pins, statuses, projects, prompts,
//! dragging threads between projects, and the new-thread selectors.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{RepoId, ThreadId};
use urushi_ui::Point;

use super::{attached, bodies, named, workspace};
use crate::keys::Named;
use crate::msg::{Effect, Followup, Msg};
use crate::organize::{
    Dragged, DropTarget, MenuPage, MenuTarget, Prompt, Select, SidebarTab, section_key,
};
use crate::state::{AppState, Dialog};

fn thread() -> String {
    ThreadId::from_u128(10).to_string()
}

fn repo() -> String {
    RepoId::from_u128(1).to_string()
}

/// The sample workspace plus a project "Billing" (id `f1`).
fn with_project() -> AppState {
    let mut s = attached();
    let mut w = workspace();
    w.folders.push(v1::Folder {
        id: "f1".into(),
        name: "Billing".into(),
        ..Default::default()
    });
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
}

fn update_request(fx: &[Effect]) -> Option<&v1::UpdateThreadRequest> {
    bodies(fx).into_iter().find_map(|b| match b {
        Body::UpdateThread(r) => Some(r),
        _ => None,
    })
}

#[test]
fn a_secondary_click_opens_the_thread_menu_where_it_happened() {
    let mut s = with_project();
    s.update(Msg::Pointer(Point::new(40.0, 120.0)));
    s.update(Msg::OpenMenu(MenuTarget::Thread(thread())));
    let menu = s.menu.clone().unwrap();
    assert_eq!(menu.at, Point::new(40.0, 120.0));
    assert_eq!(menu.page, MenuPage::Main);

    // Pin: one request, and the menu closes.
    let fx = s.update(Msg::TogglePin(thread()));
    assert_eq!(update_request(&fx).unwrap().pinned, Some(true));
    assert!(s.menu.is_none());

    // Escape closes an open menu without touching anything else.
    s.update(Msg::OpenMenu(MenuTarget::Thread(thread())));
    assert!(s.update(Msg::Key(named(Named::Escape))).is_empty());
    assert!(s.menu.is_none());
}

#[test]
fn statuses_are_picked_from_the_menu_or_typed_in_a_prompt() {
    let mut s = with_project();
    s.update(Msg::OpenMenu(MenuTarget::Thread(thread())));
    s.update(Msg::MenuPage(MenuPage::Status));
    assert_eq!(s.menu.as_ref().unwrap().page, MenuPage::Status);
    let fx = s.update(Msg::SetStatus {
        thread: thread(),
        status: None,
    });
    assert_eq!(
        update_request(&fx).unwrap().status.as_deref(),
        Some(""),
        "cleared"
    );

    s.update(Msg::OpenPrompt(Prompt::NewStatus(thread())));
    assert!(s.update(Msg::Submit).is_empty(), "blank does nothing");
    s.update(Msg::Paste("In review".into()));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert_eq!(
        update_request(&fx).unwrap().status.as_deref(),
        Some("In review")
    );
}

#[test]
fn renaming_prefills_the_current_name() {
    let mut s = with_project();
    s.update(Msg::OpenPrompt(Prompt::RenameThread(thread())));
    let Some(Dialog::Prompt { field, .. }) = &s.dialog else {
        panic!("no prompt");
    };
    assert_eq!(field.text(), "Fix bug");
    s.update(Msg::Paste(" 2".into()));
    let fx = s.update(Msg::Submit);
    assert_eq!(
        update_request(&fx).unwrap().title.as_deref(),
        Some("Fix bug 2")
    );

    s.update(Msg::OpenPrompt(Prompt::RenameProject("f1".into())));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::RenameFolder(v1::RenameFolderRequest { name, .. })] if name == "Billing")
    );
}

#[test]
fn a_new_project_from_the_move_menu_takes_the_thread_with_it() {
    let mut s = with_project();
    s.update(Msg::OpenPrompt(Prompt::NewProject(Some(thread()))));
    s.update(Msg::Paste("Launch".into()));
    let fx = s.update(Msg::Submit);
    assert!(matches!(
        &fx[..],
        [Effect::Request { body: Body::CreateFolder(_), followup: Followup::ProjectCreated(Some(t)) }] if *t == thread()
    ));
    let fx = s.update(Msg::Response {
        followup: Followup::ProjectCreated(Some(thread())),
        result: Ok(Outcome::Folder(v1::Folder {
            id: "f2".into(),
            name: "Launch".into(),
            ..Default::default()
        })),
    });
    assert!(s.dialog.is_none());
    assert_eq!(
        update_request(&fx).unwrap().folder_id.as_deref(),
        Some("f2")
    );

    let fx = s.update(Msg::DeleteProject("f1".into()));
    assert!(matches!(bodies(&fx)[..], [Body::DeleteFolder(_)]));
}

#[test]
fn dragging_a_thread_onto_a_project_moves_it() {
    let mut s = with_project();
    s.update(Msg::Pointer(Point::new(10.0, 10.0)));
    s.update(Msg::DragThread(thread()));
    s.update(Msg::DragMove(Point::new(30.0, 80.0)));
    assert_eq!(
        s.dragging,
        Some((Dragged::Thread(thread()), Point::new(30.0, 80.0)))
    );
    let fx = s.update(Msg::DropThread(DropTarget::Project("f1".into())));
    assert_eq!(
        update_request(&fx).unwrap().folder_id.as_deref(),
        Some("f1")
    );
    assert!(s.dragging.is_none());

    // Dropping where it already is sends nothing; cancelling just ends the drag.
    s.update(Msg::DragThread(thread()));
    assert!(s.update(Msg::DropThread(DropTarget::Ungrouped)).is_empty());
    s.update(Msg::DragThread(thread()));
    s.update(Msg::DragCancel);
    assert!(s.dragging.is_none());
}

#[test]
fn sections_collapse_and_tabs_switch() {
    let mut s = with_project();
    let key = section_key(Some("f1"), Some("Blocked"));
    s.update(Msg::ToggleSection(key.clone()));
    assert!(s.collapsed.contains(&key));
    s.update(Msg::ToggleSection(key.clone()));
    assert!(!s.collapsed.contains(&key));
    s.update(Msg::SidebarTab(SidebarTab::Repos));
    assert_eq!(s.sidebar_tab, SidebarTab::Repos);
}

#[test]
fn the_new_thread_dialog_picks_repo_and_base_from_lists() {
    let mut s = with_project();
    let fx = s.update(Msg::OpenNewThread {
        repo: None,
        project: Some("f1".into()),
    });
    assert!(
        matches!(&fx[..], [Effect::Request { body: Body::ListBranches(r), followup: Followup::Branches(id) }] if r.project_id == repo() && *id == repo()),
        "{fx:?}"
    );
    s.update(Msg::Response {
        followup: Followup::Branches(repo()),
        result: Ok(Outcome::Branches(v1::Branches {
            branches: vec!["main".into(), "dev".into()],
            current: "main".into(),
        })),
    });
    s.update(Msg::ToggleSelect(Select::Base));
    assert!(matches!(
        &s.dialog,
        Some(Dialog::NewThread {
            open: Some(Select::Base),
            ..
        })
    ));
    s.update(Msg::PickBase(Some("dev".into())));
    assert!(
        matches!(&s.dialog, Some(Dialog::NewThread { open: None, base: Some(b), .. }) if b == "dev")
    );

    // Switching repos resets the base and loads that repo's branches.
    let fx = s.update(Msg::PickRepo("other".into()));
    assert!(
        matches!(&s.dialog, Some(Dialog::NewThread { base: None, repo, .. }) if repo == "other")
    );
    assert!(matches!(bodies(&fx)[..], [Body::ListBranches(_)]));
    s.update(Msg::PickRepo(repo()));
    s.update(Msg::PickBase(Some("dev".into())));

    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateThread(v1::CreateThreadRequest { project_id, base_ref, folder_id, .. })] if *project_id == repo() && base_ref == "dev" && folder_id == "f1"),
        "{fx:?}"
    );
}

#[test]
fn a_new_thread_without_repos_asks_to_add_one() {
    let mut s = AppState::new(crate::config::ConnectTarget::Local(None), super::CELL);
    s.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: v1::Workspace::default(),
    });
    s.update(Msg::OpenNewThread {
        repo: None,
        project: None,
    });
    assert!(matches!(s.dialog, Some(Dialog::AddProject { .. })));
    assert_eq!(s.sidebar_tab, SidebarTab::Repos);
}

#[test]
fn the_new_thread_dialog_picks_the_project_or_none() {
    let mut s = with_project();
    s.update(Msg::OpenNewThread {
        repo: None,
        project: None,
    });
    s.update(Msg::ToggleSelect(Select::Project));
    assert!(matches!(
        &s.dialog,
        Some(Dialog::NewThread {
            open: Some(Select::Project),
            ..
        })
    ));
    s.update(Msg::PickProject(Some("f1".into())));
    assert!(
        matches!(&s.dialog, Some(Dialog::NewThread { open: None, project: Some(p), .. }) if p == "f1")
    );
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateThread(v1::CreateThreadRequest { folder_id, .. })] if folder_id == "f1"),
        "{fx:?}"
    );

    // None: the thread is ungrouped.
    s.update(Msg::OpenNewThread {
        repo: None,
        project: Some("f1".into()),
    });
    s.update(Msg::PickProject(None));
    assert!(matches!(
        &s.dialog,
        Some(Dialog::NewThread {
            project: None,
            open: None,
            ..
        })
    ));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateThread(v1::CreateThreadRequest { folder_id, .. })] if folder_id.is_empty()),
        "{fx:?}"
    );
}
