//! Overlays drawn above everything: the context menu (opened by a secondary click on a
//! thread, a project or a text field) and the label that follows the pointer while dragging a thread.

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Point, Role, Theme, key};

use crate::msg::Msg;
use crate::organize::{ContextMenu, MenuPage, MenuTarget, Prompt};
use crate::state::{AppState, FieldCommand};

const MENU_WIDTH: f32 = 230.0;

/// The context menu and its click-away backdrop, if a menu is open.
pub(super) fn overlay(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    let Some(menu) = &state.menu else {
        return Vec::new();
    };
    let items = match &menu.target {
        MenuTarget::Thread(id) => thread_items(state, theme, menu, id),
        MenuTarget::Project(id) => project_items(theme, id),
        MenuTarget::Field(_) => field_items(state, theme),
    };
    let backdrop = Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .on_click(Msg::CloseMenu)
        .on_secondary_click(Msg::CloseMenu);
    let mut backdrop = backdrop;
    backdrop.cursor = urushi_ui::CursorIcon::Default;
    let card = Node::column()
        .width(Length::Fixed(MENU_WIDTH))
        .padding(Insets::all(4.0))
        .gap(1.0)
        .background(theme.surface)
        .border(1.0, theme.border)
        .radius(theme.radius + 2.0)
        .key(key(&["context-menu"]))
        .role(Role::Dialog, "Menu")
        .children(items)
        .at(menu.at);
    vec![backdrop, card]
}

/// The dragged thread's title, next to the pointer.
pub(super) fn drag_ghost(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    let (thread, at) = state.dragging.as_ref()?;
    let title = state.workspace.thread(thread)?.title.clone();
    Some(
        Node::row()
            .padding(Insets::symmetric(5.0, 10.0))
            .background(theme.surface)
            .border(1.0, theme.accent)
            .radius(theme.radius)
            .child(Node::text(
                title,
                Font::UiMedium,
                theme.font_size,
                theme.text,
            ))
            .at(Point::new(at.x + 14.0, at.y + 10.0)),
    )
}

/// One menu row. `trailing` is a check mark, chevron or hint on the right.
fn item(theme: &Theme, label: &str, trailing: &str, color: Color, msg: Msg) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(28.0))
        .padding(Insets::symmetric(0.0, 10.0))
        .gap(8.0)
        .radius(theme.radius)
        .hover_background(theme.hover)
        .key(key(&["menu-item", label]))
        .on_click(msg)
        .role(Role::Button, label)
        .child(Node::text(label, Font::Ui, theme.font_size, color).width(Length::Fill(1.0)))
        .child(Node::text(
            trailing,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
}

fn separator(theme: &Theme) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets::symmetric(3.0, 6.0))
        .child(urushi_ui::widgets::divider(theme, false))
}

/// A sub-page's title row, which goes back to the main page.
fn back(theme: &Theme, title: &str) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(28.0))
        .padding(Insets::symmetric(0.0, 8.0))
        .gap(6.0)
        .radius(theme.radius)
        .hover_background(theme.hover)
        .align(Align::Center)
        .on_click(Msg::MenuPage(MenuPage::Main))
        .role(Role::Button, format!("Back from {title}"))
        .child(Node::text(
            "‹",
            Font::UiMedium,
            theme.font_size + 2.0,
            theme.text_muted,
        ))
        .child(Node::text(
            title,
            Font::UiMedium,
            theme.font_size_small,
            theme.text_muted,
        ))
}

/// Cut, copy, paste and select all, for a text field. Cut and copy are dimmed with
/// nothing selected.
fn field_items(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    let selected = state.field().is_some_and(|f| f.selection().is_some());
    let hint = |letter: &str| {
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            format!("⌘{letter}")
        } else {
            format!("Ctrl+{letter}")
        }
    };
    let dim = |on: bool| if on { theme.text } else { theme.text_muted };
    vec![
        item(
            theme,
            "Cut",
            &hint("X"),
            dim(selected),
            Msg::EditField(FieldCommand::Cut),
        ),
        item(
            theme,
            "Copy",
            &hint("C"),
            dim(selected),
            Msg::EditField(FieldCommand::Copy),
        ),
        item(
            theme,
            "Paste",
            &hint("V"),
            theme.text,
            Msg::EditField(FieldCommand::Paste),
        ),
        separator(theme),
        item(
            theme,
            "Select all",
            &hint("A"),
            theme.text,
            Msg::EditField(FieldCommand::SelectAll),
        ),
    ]
}

fn check(on: bool) -> &'static str {
    if on { "✓" } else { "" }
}

fn thread_items(state: &AppState, theme: &Theme, menu: &ContextMenu, id: &str) -> Vec<Node<Msg>> {
    let Some(thread) = state.workspace.thread(id) else {
        return Vec::new();
    };
    let id = id.to_owned();
    match menu.page {
        MenuPage::Main => vec![
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
            separator(theme),
            item(
                theme,
                "Delete thread…",
                "",
                theme.danger,
                Msg::RequestDeleteThread(id),
            ),
        ],
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

fn project_items(theme: &Theme, id: &str) -> Vec<Node<Msg>> {
    vec![
        item(
            theme,
            "New thread…",
            "",
            theme.text,
            Msg::OpenNewThread {
                repo: None,
                project: Some(id.to_owned()),
            },
        ),
        item(
            theme,
            "Rename…",
            "",
            theme.text,
            Msg::OpenPrompt(Prompt::RenameProject(id.to_owned())),
        ),
        separator(theme),
        item(
            theme,
            "Delete project",
            "threads are kept",
            theme.danger,
            Msg::DeleteProject(id.to_owned()),
        ),
    ]
}
