use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{Layout, PaneId, RepoId, TabId, ThreadId};
use urushi_proto::convert;
use urushi_ui::{CellMetrics, Rect};

use crate::config::ConnectTarget;
use crate::keys::{KeyPress, Mods, Named};
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Dialog};

pub(super) const CELL: CellMetrics = CellMetrics {
    width: 8.0,
    height: 16.0,
};

pub(super) fn id(n: u128) -> String {
    PaneId::from_u128(n).to_string()
}

pub(super) fn workspace() -> v1::Workspace {
    let pane = PaneId::from_u128(100);
    let layout = convert::layout_to_proto(&Layout::single(pane));
    v1::Workspace {
        revision: 1,
        projects: vec![v1::Project {
            id: RepoId::from_u128(1).to_string(),
            name: "repo".into(),
            ..Default::default()
        }],
        threads: vec![v1::Thread {
            id: ThreadId::from_u128(10).to_string(),
            project_id: RepoId::from_u128(1).to_string(),
            title: "Fix bug".into(),
            primary_branch: "urushi/fix-bug-1".into(),
            state: v1::ThreadState::Ready.into(),
            tab_ids: vec![TabId::from_u128(20).to_string()],
            ..Default::default()
        }],
        tabs: vec![v1::Tab {
            id: TabId::from_u128(20).to_string(),
            thread_id: ThreadId::from_u128(10).to_string(),
            title: "Terminal".into(),
            layout: Some(layout),
            focused_pane_id: pane.to_string(),
            attention: Vec::new(),
        }],
        folders: Vec::new(),
        boards: Vec::new(),
        quick_terminal: None,
    }
}

pub(super) fn connected() -> AppState {
    let mut s = AppState::new(ConnectTarget::Local(None), CELL);
    s.update(Msg::Connected {
        welcome: v1::Welcome {
            host_name: "box".into(),
            ..Default::default()
        },
        workspace: workspace(),
    });
    s
}

pub(super) fn bodies(fx: &[Effect]) -> Vec<&Body> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::Request { body, .. } = e {
                Some(body)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn key(character: &str, mods: Mods) -> KeyPress {
    KeyPress {
        named: None,
        character: Some(character.into()),
        text: Some(character.into()),
        mods,
    }
}

pub(super) fn named(n: Named) -> KeyPress {
    KeyPress {
        named: Some(n),
        character: None,
        text: None,
        mods: Mods::default(),
    }
}

pub(super) fn shortcut() -> Mods {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Mods {
            logo: true,
            ..Mods::default()
        }
    } else {
        Mods {
            ctrl: true,
            shift: true,
            ..Mods::default()
        }
    }
}

pub(super) fn attached() -> AppState {
    let mut s = connected();
    let fx = s.update(Msg::AreaChanged(Rect::new(0.0, 0.0, 816.0, 400.0)));
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::AttachPane(v1::AttachPaneRequest {
                cols: 100,
                rows: 24,
                ..
            })]
        ),
        "{fx:?}"
    );
    let update = v1::ScreenUpdate {
        pane_id: id(100),
        seq: 1,
        full: true,
        cols: 100,
        rows: 24,
        ..Default::default()
    };
    let fx = s.update(Msg::Response {
        followup: Followup::Attached(id(100)),
        result: Ok(Outcome::Screen(update)),
    });
    assert!(matches!(
        &fx[..],
        [Effect::Notify(v1::notification::Body::AckScreen(
            v1::AckScreen { seq: 1, .. }
        ))]
    ));
    s
}

#[test]
fn connecting_selects_the_first_thread_and_loads_settings() {
    let mut s = AppState::new(ConnectTarget::Local(None), CELL);
    let fx = s.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: workspace(),
    });
    assert_eq!(
        s.selected_thread.as_deref(),
        Some(ThreadId::from_u128(10).to_string().as_str())
    );
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::GetSettings(_)))
    );
    assert!(
        !bodies(&fx).iter().any(|b| matches!(b, Body::AttachPane(_))),
        "no attach before the area is known"
    );
}

#[test]
fn reopening_shows_the_remembered_thread_and_tab() {
    let mut ws = workspace();
    let mut other = ws.threads[0].clone();
    other.id = ThreadId::from_u128(11).to_string();
    other.tab_ids = vec![
        TabId::from_u128(21).to_string(),
        TabId::from_u128(22).to_string(),
    ];
    ws.threads.push(other.clone());
    for tab in [21, 22] {
        let mut t = ws.tabs[0].clone();
        t.id = TabId::from_u128(tab).to_string();
        t.thread_id.clone_from(&other.id);
        ws.tabs.push(t);
    }
    let remembered = crate::ui_state::UiState {
        thread: Some(other.id.clone()),
        tabs: [(other.id.clone(), TabId::from_u128(22).to_string())].into(),
        ..Default::default()
    };
    let mut s = AppState::new(ConnectTarget::Local(None), CELL);
    remembered.clone().apply(&mut s);
    s.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: ws.clone(),
    });
    assert_eq!(s.selected_thread.as_deref(), Some(other.id.as_str()));
    assert_eq!(
        s.current_tab().map(|t| t.id.clone()),
        Some(TabId::from_u128(22).to_string())
    );
    assert_eq!(crate::ui_state::UiState::of(&s), remembered);

    // A thread deleted meanwhile: the first thread is shown instead.
    let mut s = AppState::new(ConnectTarget::Local(None), CELL);
    crate::ui_state::UiState {
        thread: Some(ThreadId::from_u128(99).to_string()),
        ..Default::default()
    }
    .apply(&mut s);
    s.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: ws,
    });
    assert_eq!(
        s.selected_thread.as_deref(),
        Some(ThreadId::from_u128(10).to_string().as_str())
    );
}

#[test]
fn panes_attach_resize_and_receive_updates() {
    let mut s = attached();
    assert!(s.screens.contains_key(&id(100)));
    // Resizing the window resizes the terminal.
    let fx = s.update(Msg::AreaChanged(Rect::new(0.0, 0.0, 416.0, 400.0)));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::SetPaneSize(v1::SetPaneSizeRequest { cols: 50, .. })]
    ));
    // Screen updates are applied and acknowledged.
    let update = v1::ScreenUpdate {
        pane_id: id(100),
        seq: 2,
        cols: 50,
        rows: 24,
        lines: vec![v1::ScreenLine {
            index: 0,
            spans: vec![v1::Span {
                text: "hi".into(),
                style: None,
            }],
        }],
        ..Default::default()
    };
    let fx = s.update(Msg::Daemon(v1::event::Body::ScreenUpdate(update)));
    assert_eq!(s.screens[&id(100)].line_text(0), "hi");
    assert!(matches!(
        &fx[..],
        [Effect::Notify(v1::notification::Body::AckScreen(
            v1::AckScreen { seq: 2, .. }
        ))]
    ));
}

#[test]
fn typing_goes_to_the_focused_pane_and_shortcuts_do_not() {
    let mut s = attached();
    let fx = s.update(Msg::Key(key("a", Mods::default())));
    assert!(
        matches!(&fx[..], [Effect::Notify(v1::notification::Body::Input(v1::Input { kind: Some(v1::input::Kind::Text(t)), .. }))] if t == "a")
    );
    // The new-tab shortcut asks what to run; Enter picks a terminal.
    s.update(Msg::Key(key("t", shortcut())));
    assert!(matches!(s.dialog, Some(Dialog::NewTab { .. })));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert!(matches!(bodies(&fx)[..], [Body::CreateTab(_)]));
    let fx = s.update(Msg::Key(key("d", shortcut())));
    assert!(
        matches!(bodies(&fx)[..], [Body::SplitPane(v1::SplitPaneRequest { side, .. })] if *side == v1::Direction::Right as i32)
    );
}

#[test]
fn deleting_a_thread_always_goes_through_the_warning() {
    let mut s = attached();
    let thread = ThreadId::from_u128(10).to_string();
    let fx = s.update(Msg::RequestDeleteThread(thread.clone()));
    assert!(
        matches!(bodies(&fx)[..], [Body::PlanThreadDeletion(_)]),
        "asks for a plan first, never deletes directly"
    );

    let plan = v1::DeletionPlan {
        thread_id: thread.clone(),
        token: "tok".into(),
        local_branches: vec!["urushi/fix-bug-1".into()],
        ..Default::default()
    };
    s.update(Msg::Response {
        followup: Followup::DeletionPlan(None),
        result: Ok(Outcome::DeletionPlan(plan)),
    });
    assert!(matches!(&s.dialog, Some(Dialog::DeleteThread { title, .. }) if title == "Fix bug"));

    // Enter does not confirm a deletion.
    assert!(s.update(Msg::Key(named(Named::Enter))).is_empty());
    let fx = s.update(Msg::ConfirmDelete);
    // Its terminal is detached, and the deletion asked for once.
    let deletes: Vec<_> = bodies(&fx)
        .into_iter()
        .filter(|b| matches!(b, Body::DeleteThread(_)))
        .collect();
    assert!(
        matches!(deletes[..], [Body::DeleteThread(v1::DeleteThreadRequest { token, .. })] if token == "tok"),
        "{fx:?}"
    );
    assert!(
        fx.iter().any(|e| matches!(
            e,
            Effect::Request { followup: Followup::ThreadDeleted(t), .. } if *t == thread
        )),
        "the answer goes to the deletion, not to whatever dialog is open then"
    );
    // The warning closes and the thread is gone at once (UR-8); deletion runs on.
    assert_eq!(s.dialog, None);
    assert!(s.workspace.thread(&thread).is_none());
    assert!(s.update(Msg::ConfirmDelete).is_empty(), "no double submit");
}

#[test]
fn new_thread_dialog_edits_fields_and_submits() {
    let mut s = attached();
    s.update(Msg::OpenNewThread {
        repo: Some(RepoId::from_u128(1).to_string()),
        project: None,
    });
    for c in ["F", "i", "x"] {
        s.update(Msg::Key(key(c, Mods::default())));
    }
    s.update(Msg::Key(named(Named::Backspace)));
    // Start from is chosen from the repo's branches.
    s.update(Msg::ToggleSelect(crate::organize::Select::Base));
    s.update(Msg::PickBase(Some("main".into())));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateThread(v1::CreateThreadRequest { title, base_ref, branch, skip_initial_tab: true, .. })] if title == "Fi" && base_ref == "main" && branch == "urushi/fi"),
        "{fx:?}"
    );
    let created = v1::Thread {
        id: id(77),
        ..Default::default()
    };
    s.update(Msg::Response {
        followup: Followup::ThreadCreated,
        result: Ok(Outcome::Thread(created)),
    });
    assert_eq!(s.selected_thread, Some(id(77)));
    assert!(s.dialog.is_none());
}

#[test]
fn escape_closes_dialogs_except_a_required_connect() {
    let mut s = attached();
    s.update(Msg::OpenAddProject);
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.dialog.is_none());

    let mut mobile = AppState::new(ConnectTarget::Ask, CELL);
    mobile.update(Msg::Key(named(Named::Escape)));
    assert!(matches!(mobile.dialog, Some(Dialog::Connect { .. })));
    mobile.update(Msg::Text("box.tail.ts.net".into()));
    let fx = mobile.update(Msg::Submit);
    assert!(
        matches!(&fx[..], [Effect::Reconnect(ConnectTarget::Remote(h))] if h == "box.tail.ts.net")
    );
}

#[test]
fn clicking_focuses_and_dragging_selects() {
    let mut s = attached();
    let fx = s.update(Msg::AreaPress(urushi_ui::Point::new(20.0, 20.0)));
    assert!(fx.is_empty(), "already focused");
    s.update(Msg::AreaDrag(urushi_ui::Point::new(60.0, 20.0)));
    let (pane, selection) = s.selection.clone().unwrap();
    assert_eq!(pane, id(100));
    assert!(selection.head.1 > selection.anchor.1);
    let fx = s.update(Msg::Copy);
    assert!(matches!(&fx[..], [Effect::SetClipboard(_)]));
}

#[test]
fn compact_layout_navigates_between_threads_and_terminals() {
    let mut s = attached();
    s.update(Msg::SetCompact(true));
    s.update(Msg::ShowSidebar);
    assert!(s.sidebar_open);
    s.update(Msg::SelectThread(ThreadId::from_u128(10).to_string()));
    assert!(!s.sidebar_open, "choosing a thread shows its terminals");
}

mod attachments;
mod attention;
mod board;
mod board_order;
mod daemon;
mod deletion;
mod fields;
mod flows;
mod jira;
mod labels;
mod organize;
mod panes;
mod pointer;
mod project_order;
mod project_settings;
mod quick;
mod setup;
mod sidebar;
mod subtask_drafts;
mod task_in_thread;
mod task_message;
mod tasks;
mod thread_order;
