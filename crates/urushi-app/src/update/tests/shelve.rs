//! Shelving a thread (protocol 1.34, ADR 0060): the menu offers Shelve and Unshelve (not
//! to older daemons), shelving asks first while an agent works, the card says what is
//! happening until the workspace shows it, and a shelved thread shows Unshelve instead
//! of its terminals, which are detached.

use urushi_client::v1::{self, request::Body};
use urushi_ui::{Role, Theme};

use super::{attached, bodies, id, workspace};
use crate::msg::{Effect, Followup, Msg, ThreadMsg};
use crate::organize::{MenuTarget, SHELVING_SINCE_MINOR};
use crate::state::{AppState, Dialog};

fn thread_id() -> String {
    id(10)
}

/// The sample workspace, attached to its one pane, from a daemon that shelves.
fn shelving() -> AppState {
    let mut s = attached();
    s.daemon_minor = SHELVING_SINCE_MINOR;
    s
}

/// The sample workspace with its thread shelved (or not), and maybe an agent running.
fn workspace_with(shelved: bool, running: bool) -> v1::Workspace {
    let mut w = workspace();
    w.threads[0].shelved = shelved;
    if running {
        w.tabs[0].running.push(v1::PaneRunning {
            pane_id: id(100),
            since_ms: 1,
        });
    }
    w
}

fn shelve_requests(fx: &[Effect]) -> Vec<v1::SetThreadShelvedRequest> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::SetThreadShelved(r) => Some(r.clone()),
            _ => None,
        })
        .collect()
}

fn render(s: &AppState, width: f32) -> urushi_ui::Frame<Msg> {
    let mut text = urushi_ui::TextSystem::bundled_only();
    let root = crate::view::view(s, &Theme::dark(), &std::collections::HashMap::new());
    urushi_ui::build(
        &root,
        urushi_ui::Rect::new(0.0, 0.0, width, 800.0),
        &mut text,
        None,
    )
}

fn buttons(s: &AppState) -> Vec<String> {
    render(s, 1200.0)
        .access
        .into_iter()
        .filter(|a| a.role == Role::Button)
        .map(|a| a.label)
        .collect()
}

#[test]
fn the_menu_shelves_and_unshelves_and_older_daemons_get_neither() {
    let mut s = shelving();
    s.update(Msg::OpenMenu(MenuTarget::Thread(thread_id())));
    assert!(buttons(&s).contains(&"Shelve".to_owned()));
    let fx = s.update(Msg::Thread(ThreadMsg::Shelve {
        thread: thread_id(),
        confirmed: false,
    }));
    assert!(s.menu.is_none());
    let sent = shelve_requests(&fx);
    assert_eq!(sent.len(), 1, "{fx:?}");
    assert_eq!(
        (sent[0].thread_id.clone(), sent[0].shelved),
        (thread_id(), true)
    );
    // Asked once: a second Shelve while the first runs sends nothing, and the menu
    // has nothing to offer.
    let again = s.update(Msg::Thread(ThreadMsg::Shelve {
        thread: thread_id(),
        confirmed: false,
    }));
    assert!(shelve_requests(&again).is_empty());
    s.update(Msg::OpenMenu(MenuTarget::Thread(thread_id())));
    let labels = buttons(&s);
    assert!(
        !labels.iter().any(|l| l == "Shelve" || l == "Unshelve"),
        "{labels:?}"
    );
    s.update(Msg::CloseMenu);

    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(
        workspace_with(true, false),
    )));
    assert!(s.shelving.is_empty(), "done once the workspace shows it");
    s.update(Msg::OpenMenu(MenuTarget::Thread(thread_id())));
    assert!(buttons(&s).contains(&"Unshelve".to_owned()));
    let fx = s.update(Msg::Thread(ThreadMsg::Unshelve(thread_id())));
    let sent = shelve_requests(&fx);
    assert_eq!(
        (sent[0].thread_id.clone(), sent[0].shelved),
        (thread_id(), false)
    );

    let mut old = attached();
    old.daemon_minor = SHELVING_SINCE_MINOR - 1;
    old.update(Msg::OpenMenu(MenuTarget::Thread(thread_id())));
    assert!(!buttons(&old).contains(&"Shelve".to_owned()));
    let fx = old.update(Msg::Thread(ThreadMsg::Shelve {
        thread: thread_id(),
        confirmed: true,
    }));
    assert!(shelve_requests(&fx).is_empty(), "{fx:?}");
}

#[test]
fn shelving_asks_first_while_an_agent_works() {
    let mut s = shelving();
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(
        workspace_with(false, true),
    )));
    let fx = s.update(Msg::Thread(ThreadMsg::Shelve {
        thread: thread_id(),
        confirmed: false,
    }));
    assert!(shelve_requests(&fx).is_empty());
    assert!(matches!(&s.dialog, Some(Dialog::ShelveThread { thread }) if *thread == thread_id()));
    assert!(
        render(&s, 1200.0)
            .scene
            .dump()
            .contains("An agent is working in this thread")
    );
    let fx = s.update(Msg::Thread(ThreadMsg::Shelve {
        thread: thread_id(),
        confirmed: true,
    }));
    assert_eq!(shelve_requests(&fx).len(), 1);
    assert!(s.dialog.is_none());
}

#[test]
fn a_refused_shelve_shows_why_and_the_card_goes_back() {
    let mut s = shelving();
    s.update(Msg::Thread(ThreadMsg::Shelve {
        thread: thread_id(),
        confirmed: false,
    }));
    assert!(render(&s, 1200.0).scene.dump().contains("\"shelving…\""));
    s.update(Msg::Response {
        followup: Followup::Shelving(thread_id()),
        result: Err("can't shelve a thread that is being deleted".into()),
    });
    assert!(s.shelving.is_empty());
    assert_eq!(
        s.error.as_deref(),
        Some("can't shelve a thread that is being deleted")
    );
    assert!(!render(&s, 1200.0).scene.dump().contains("\"shelving…\""));
}

#[test]
fn a_shelved_thread_shows_unshelve_instead_of_its_terminals() {
    let mut s = shelving();
    let fx = s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(
        workspace_with(true, false),
    )));
    // Its pane is let go: the daemon runs nothing in it.
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::DetachPane(r) if r.pane_id == id(100))),
        "{fx:?}"
    );
    assert!(s.current_tab().is_none());
    assert!(s.pane_rects().is_empty());
    let dump = render(&s, 1200.0).scene.dump();
    assert!(dump.contains("is shelved"), "{dump}");
    assert!(dump.contains("\"shelved\""), "the card says so: {dump}");
    assert!(buttons(&s).contains(&"Unshelve".to_owned()));
    // On a phone too, with the way back to the threads.
    s.compact = true;
    s.sidebar_open = false;
    let phone = render(&s, 402.0);
    let labels: Vec<_> = phone.access.iter().map(|a| a.label.as_str()).collect();
    assert!(
        labels.contains(&"Unshelve") && labels.contains(&"Back to threads"),
        "{labels:?}"
    );
    s.compact = false;
    // No new tab opens in it.
    s.update(Msg::NewTab);
    assert!(s.dialog.is_none());
    // Unshelve, then the panes come back once the workspace shows it ready.
    let fx = s.update(Msg::Thread(ThreadMsg::Unshelve(thread_id())));
    assert_eq!(shelve_requests(&fx).len(), 1);
    assert!(render(&s, 1200.0).scene.dump().contains("Unshelving…"));
    let fx = s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(
        workspace_with(false, false),
    )));
    assert!(s.shelving.is_empty());
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::AttachPane(r) if r.pane_id == id(100))),
        "{fx:?}"
    );
}
