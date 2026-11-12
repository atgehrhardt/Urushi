//! A thread's context menu: rename, pin, unread, notes, status ›, move to project ›,
//! move up and down, delete. A project's parent-branch thread gets a shorter one.

use urushi_client::v1;
use urushi_ui::{Node, Theme};

use super::{back, check, item, order, separator};
use crate::msg::{Msg, ThreadMsg};
use crate::organize::{ContextMenu, MenuPage, Prompt};
use crate::state::AppState;

/// Mark as unread / read and Notes… (protocol 1.27, ADR 0048); none for older daemons.
fn extras(state: &AppState, theme: &Theme, thread: &v1::Thread, id: &str) -> Vec<Node<Msg>> {
    if !state.daemon_supports_thread_extras() {
        return Vec::new();
    }
    vec![
        item(
            theme,
            if thread.unread {
                "Mark as read"
            } else {
                "Mark as unread"
            },
            "",
            theme.text,
            Msg::Thread(ThreadMsg::SetUnread {
                thread: id.to_owned(),
                unread: !thread.unread,
            }),
        ),
        item(
            theme,
            if thread.notes.is_empty() {
                "Add notes…"
            } else {
                "Edit notes…"
            },
            "",
            theme.text,
            Msg::Thread(ThreadMsg::OpenNotes(id.to_owned())),
        ),
    ]
}

pub(super) fn thread_items(
    state: &AppState,
    theme: &Theme,
    menu: &ContextMenu,
    id: &str,
) -> Vec<Node<Msg>> {
    let Some(thread) = state.workspace.thread(id) else {
        return Vec::new();
    };
    let id = id.to_owned();
    let extras = extras(state, theme, thread, &id);
    if thread.project_base {
        return base_thread_items(theme, id, extras);
    }
    match menu.page {
        MenuPage::Main | MenuPage::Color | MenuPage::Filter(_) => vec![
            item(
                theme,
                "Rename…",
                "",
                theme.text,
                Msg::OpenPrompt(Prompt::RenameThread(id.clone())),
            ),
            item(
                theme,
                if thread.pinned { "Unpin" } else { "Pin" },
                "",
                theme.text,
                Msg::TogglePin(id.clone()),
            ),
            item(
                theme,
                "Status",
                &format!("{}  ›", thread.status),
                theme.text,
                Msg::MenuPage(MenuPage::Status),
            ),
            item(
                theme,
                "Move to project",
                &format!(
                    "{}  ›",
                    state
                        .workspace
                        .project(&thread.folder_id)
                        .map_or("", |p| p.name.as_str())
                ),
                theme.text,
                Msg::MenuPage(MenuPage::Move),
            ),
        ]
        .into_iter()
        .chain(extras)
        .chain(order::thread_moves(state, theme, &id))
        .chain([
            separator(theme),
            item(
                theme,
                "Delete thread…",
                "",
                theme.danger,
                Msg::RequestDeleteThread(id),
            ),
        ])
        .collect(),
        MenuPage::Status => status_page(state, theme, thread, id),
        MenuPage::Move => move_page(state, theme, thread, id),
    }
}

fn status_page(state: &AppState, theme: &Theme, thread: &v1::Thread, id: String) -> Vec<Node<Msg>> {
    let mut items = vec![back(theme, "Status")];
    items.extend(state.workspace.statuses().into_iter().map(|status| {
        let current = thread.status == status;
        item(
            theme,
            &status,
            check(current),
            theme.text,
            Msg::SetStatus {
                thread: id.clone(),
                status: Some(status.clone()),
            },
        )
    }));
    items.push(item(
        theme,
        "New status…",
        "",
        theme.text,
        Msg::OpenPrompt(Prompt::NewStatus(id.clone())),
    ));
    if !thread.status.is_empty() {
        items.push(item(
            theme,
            "Clear status",
            "",
            theme.text_muted,
            Msg::SetStatus {
                thread: id,
                status: None,
            },
        ));
    }
    items
}

fn move_page(state: &AppState, theme: &Theme, thread: &v1::Thread, id: String) -> Vec<Node<Msg>> {
    let current = state
        .workspace
        .project(&thread.folder_id)
        .map(|p| p.id.clone());
    let mut items = vec![back(theme, "Move to project")];
    items.extend(state.workspace.projects().iter().map(|p| {
        item(
            theme,
            &p.name,
            check(current.as_deref() == Some(&p.id)),
            theme.text,
            Msg::MoveThread {
                thread: id.clone(),
                project: Some(p.id.clone()),
            },
        )
    }));
    items.push(item(
        theme,
        "No project",
        check(current.is_none()),
        theme.text,
        Msg::MoveThread {
            thread: id.clone(),
            project: None,
        },
    ));
    items.push(item(
        theme,
        "New project…",
        "",
        theme.text,
        Msg::OpenPrompt(Prompt::NewProject(Some(id))),
    ));
    items
}

/// A project's parent-branch thread is always at the project's top and stays there:
/// it can be renamed or closed (which switches it off in the project's settings).
fn base_thread_items(theme: &Theme, id: String, extras: Vec<Node<Msg>>) -> Vec<Node<Msg>> {
    [item(
        theme,
        "Rename…",
        "",
        theme.text,
        Msg::OpenPrompt(Prompt::RenameThread(id.clone())),
    )]
    .into_iter()
    .chain(extras)
    .chain([
        separator(theme),
        item(
            theme,
            "Close parent-branch thread…",
            "",
            theme.danger,
            Msg::RequestDeleteThread(id),
        ),
    ])
    .collect()
}
