//! A thread's unread mark and notes, and a project's color (protocol 1.27, ADR 0048):
//! the menus offer them (not to older daemons), the requests they send, opening an unread
//! thread marking it read, the notes editor, and how the sidebar shows them.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_ui::{Role, Theme};

use super::{attached, bodies, workspace};
use crate::msg::{Effect, Followup, Msg, ThreadMsg};
use crate::organize::{MenuPage, MenuTarget, THREAD_EXTRAS_SINCE_MINOR};
use crate::state::{AppState, Dialog, Field};

/// The sample workspace ("Fix bug", ungrouped and selected) plus project `p` (teal) with
/// threads `u` (unread, with notes) and `n` (read, no notes).
fn with_extras() -> AppState {
    let mut s = attached();
    s.daemon_minor = THREAD_EXTRAS_SINCE_MINOR;
    let mut w = workspace();
    w.folders.push(v1::Folder {
        id: "p".into(),
        name: "Billing".into(),
        color: v1::LabelColor::Teal.into(),
        ..Default::default()
    });
    let repo = w.projects[0].id.clone();
    let thread = |id: &str, unread: bool, notes: &str| v1::Thread {
        id: id.into(),
        project_id: repo.clone(),
        title: id.to_uppercase(),
        folder_id: "p".into(),
        unread,
        notes: notes.into(),
        state: v1::ThreadState::Ready.into(),
        ..Default::default()
    };
    w.threads.extend([
        thread("u", true, "\n  Ask about retries  \nthen ship"),
        thread("n", false, ""),
    ]);
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
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

fn render(s: &AppState) -> urushi_ui::Frame<Msg> {
    let mut text = urushi_ui::TextSystem::bundled_only();
    let root = crate::view::view(s, &Theme::dark(), &std::collections::HashMap::new());
    urushi_ui::build(
        &root,
        urushi_ui::Rect::new(0.0, 0.0, 1200.0, 800.0),
        &mut text,
        None,
    )
}

fn buttons(s: &AppState) -> Vec<String> {
    render(s)
        .access
        .into_iter()
        .filter(|a| a.role == Role::Button)
        .map(|a| a.label)
        .collect()
}

fn has(labels: &[String], label: &str) -> bool {
    labels.iter().any(|l| l == label)
}

#[test]
fn the_thread_menu_marks_unread_and_read_and_older_daemons_get_neither() {
    let mut s = with_extras();
    s.update(Msg::OpenMenu(MenuTarget::Thread("n".into())));
    let labels = buttons(&s);
    assert!(
        has(&labels, "Mark as unread") && has(&labels, "Add notes…"),
        "{labels:?}"
    );
    let fx = s.update(Msg::Thread(ThreadMsg::SetUnread {
        thread: "n".into(),
        unread: true,
    }));
    assert!(s.menu.is_none());
    let sent = updates(&fx);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].thread_id.as_str(), sent[0].unread),
        ("n", Some(true))
    );
    assert_eq!(sent[0].notes, None);

    s.update(Msg::OpenMenu(MenuTarget::Thread("u".into())));
    let labels = buttons(&s);
    assert!(
        has(&labels, "Mark as read") && has(&labels, "Edit notes…"),
        "{labels:?}"
    );
    s.update(Msg::CloseMenu);

    s.daemon_minor = THREAD_EXTRAS_SINCE_MINOR - 1;
    s.update(Msg::OpenMenu(MenuTarget::Thread("u".into())));
    let labels = buttons(&s);
    assert!(
        ![
            "Mark as read",
            "Mark as unread",
            "Add notes…",
            "Edit notes…"
        ]
        .iter()
        .any(|l| has(&labels, l)),
        "{labels:?}"
    );
    let fx = s.update(Msg::Thread(ThreadMsg::SetUnread {
        thread: "u".into(),
        unread: false,
    }));
    assert!(fx.is_empty(), "{fx:?}");
}

#[test]
fn opening_an_unread_thread_marks_it_read_once() {
    let mut s = with_extras();
    let fx = s.update(Msg::SelectThread("u".into()));
    let sent = updates(&fx);
    assert_eq!(sent.len(), 1, "{fx:?}");
    assert_eq!(
        (sent[0].thread_id.as_str(), sent[0].unread),
        ("u", Some(false))
    );
    // A read thread sends nothing.
    let fx = s.update(Msg::SelectThread("n".into()));
    assert!(updates(&fx).is_empty(), "{fx:?}");
    // Older daemons aren't asked.
    s.daemon_minor = THREAD_EXTRAS_SINCE_MINOR - 1;
    let fx = s.update(Msg::SelectThread("u".into()));
    assert!(updates(&fx).is_empty(), "{fx:?}");
}

#[test]
fn the_notes_editor_saves_or_cancels() {
    let mut s = with_extras();
    s.update(Msg::Thread(ThreadMsg::OpenNotes("u".into())));
    let Some(Dialog::ThreadNotes { thread, notes }) = &s.dialog else {
        panic!("{:?}", s.dialog);
    };
    assert_eq!(thread, "u");
    assert_eq!(notes.text(), "\n  Ask about retries  \nthen ship");
    assert_eq!(s.focus, Some(Field::ThreadNotes));
    assert!(has(&buttons(&s), "Save"));

    // Cancel sends nothing.
    let fx = s.update(Msg::CloseDialog);
    assert!(s.dialog.is_none() && fx.is_empty(), "{fx:?}");

    // Save sends the notes without trailing whitespace, and closes once they're kept.
    s.update(Msg::Thread(ThreadMsg::OpenNotes("n".into())));
    if let Some(Dialog::ThreadNotes { notes, .. }) = &mut s.dialog {
        *notes = urushi_ui::TextField::multiline("Line one\nline two\n\n");
    }
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            &fx[..],
            [Effect::Request { body: Body::UpdateThread(r), followup: Followup::CloseDialog }]
                if r.thread_id == "n" && r.notes.as_deref() == Some("Line one\nline two")
                    && r.unread.is_none()
        ),
        "{fx:?}"
    );
    s.update(Msg::Response {
        followup: Followup::CloseDialog,
        result: Ok(Outcome::Ok(v1::Empty {})),
    });
    assert!(s.dialog.is_none());
}

#[test]
fn a_project_is_colored_from_its_menu() {
    let mut s = with_extras();
    s.update(Msg::OpenMenu(MenuTarget::Project("p".into())));
    assert!(has(&buttons(&s), "Color"), "{:?}", buttons(&s));
    s.update(Msg::MenuPage(MenuPage::Color));
    let labels = buttons(&s);
    assert!(has(&labels, "Purple") && has(&labels, "None"), "{labels:?}");
    let fx = s.update(Msg::Thread(ThreadMsg::SetProjectColor {
        project: "p".into(),
        color: v1::LabelColor::Purple.into(),
    }));
    assert!(s.menu.is_none());
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::SetFolderColor(v1::SetFolderColorRequest { folder_id, color })]
                if folder_id == "p" && *color == v1::LabelColor::Purple as i32
        ),
        "{fx:?}"
    );

    s.daemon_minor = THREAD_EXTRAS_SINCE_MINOR - 1;
    s.update(Msg::OpenMenu(MenuTarget::Project("p".into())));
    assert!(!has(&buttons(&s), "Color"));
}

#[test]
fn the_sidebar_shows_unread_threads_notes_and_project_colors() {
    let s = with_extras();
    let frame = render(&s);
    let items: Vec<&str> = frame
        .access
        .iter()
        .filter(|a| a.role == Role::Item)
        .map(|a| a.label.as_str())
        .collect();
    assert!(items.contains(&"Thread U, unread, has notes"), "{items:?}");
    assert!(items.contains(&"Thread N"), "{items:?}");

    let dump = frame.scene.dump();
    // The notes' first non-blank line shows under the card, trimmed.
    assert!(dump.contains("\"Ask about retries\""), "{dump}");
    assert!(!dump.contains("then ship"), "{dump}");
    // The unread title is semibold, with an accent dot.
    let theme = Theme::dark();
    assert!(
        dump.contains(&format!("7x7 {} r4", theme.accent.to_hex())),
        "{dump}"
    );
    // The project's teal: a dot in its header and a stripe down each of its cards.
    let teal = theme.labels[urushi_core::LabelColor::Teal.index()].to_hex();
    assert!(dump.contains(&format!("8x8 {teal}")), "{dump}");
    let stripes = dump
        .lines()
        .filter(|l| l.starts_with("quad") && l.contains(" 3x") && l.contains(&teal))
        .count();
    assert_eq!(stripes, 2, "{dump}");
}
