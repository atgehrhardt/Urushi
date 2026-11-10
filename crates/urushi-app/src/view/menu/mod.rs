//! Overlays drawn above everything: the context menu (opened by a secondary click on a
//! thread, a project, a text field, a task card, a board column, a board tab or a terminal
//! pane, or as the board's filters) and the label that follows the pointer while dragging
//! a thread, a project or a board tab.

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Point, Role, Theme, key};

use crate::msg::Msg;
use crate::organize::{ContextMenu, Dragged, MenuPage, MenuTarget, Prompt};
use crate::state::{AppState, FieldCommand};

mod boards;
mod filter;
mod jira;
mod order;
mod pane;
mod quick_filter;
mod tasks;

const MENU_WIDTH: f32 = 230.0;

/// The context menu and its click-away backdrop, if a menu is open.
pub(super) fn overlay(
    state: &AppState,
    theme: &Theme,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Vec<Node<Msg>> {
    let Some(menu) = &state.menu else {
        return Vec::new();
    };
    let items = match &menu.target {
        MenuTarget::Thread(id) => thread_items(state, theme, menu, id),
        MenuTarget::Project(id) => project_items(state, theme, id),
        MenuTarget::Field(_) => field_items(state, theme),
        MenuTarget::Task(id) => tasks::task_items(state, theme, menu, id),
        MenuTarget::Column(id) => tasks::column_items(state, theme, menu, id),
        MenuTarget::Board(id) => boards::board_items(state, theme, id),
        MenuTarget::Filter(board) => filter::filter_items(state, theme, (board, menu.page), scroll),
        MenuTarget::QuickFilter(board, filter) => {
            quick_filter::quick_filter_items(state, theme, (board, filter))
        }
        MenuTarget::NewBoard => jira::new_board_items(theme),
        MenuTarget::Pane(pane) => pane::pane_items(state, theme, pane),
    };
    let backdrop = Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .on_click(Msg::CloseMenu)
        .on_secondary_click(Msg::CloseMenu);
    let mut backdrop = backdrop;
    backdrop.cursor = urushi_ui::CursorIcon::Default;
    let card = urushi_ui::widgets::overlay(
        theme,
        Node::column()
            .width(Length::Fixed(MENU_WIDTH))
            .padding(Insets::all(4.0))
            .gap(1.0),
        theme.radius + 3.0,
    )
    .key(key(&["context-menu"]))
    .role(Role::Dialog, "Menu")
    .children(items)
    .at(menu.at);
    vec![backdrop, card]
}

/// The dragged thread's title, or project's or board's name, next to the pointer.
pub(super) fn drag_ghost(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    let (dragged, at) = state.dragging.as_ref()?;
    let title = match dragged {
        Dragged::Thread(thread) => state.workspace.thread(thread)?.title.clone(),
        Dragged::Project(project) => state.workspace.project(project)?.name.clone(),
        Dragged::Board(board) => state.workspace.board(board)?.name.clone(),
    };
    Some(
        urushi_ui::widgets::overlay(
            theme,
            Node::row().padding(Insets::symmetric(5.0, 10.0)),
            theme.radius,
        )
        .border(1.0, theme.accent)
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
    let selected = state
        .field()
        .is_some_and(|f| f.selection().is_some() && !f.is_secret());
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
    if thread.project_base {
        return base_thread_items(theme, id);
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
fn base_thread_items(theme: &Theme, id: String) -> Vec<Node<Msg>> {
    vec![
        item(
            theme,
            "Rename…",
            "",
            theme.text,
            Msg::OpenPrompt(Prompt::RenameThread(id.clone())),
        ),
        separator(theme),
        item(
            theme,
            "Close parent-branch thread…",
            "",
            theme.danger,
            Msg::RequestDeleteThread(id),
        ),
    ]
}

fn project_items(state: &AppState, theme: &Theme, id: &str) -> Vec<Node<Msg>> {
    let mut items = vec![
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
    ];
    if state.daemon_supports_project_settings() {
        items.push(item(
            theme,
            "Project settings…",
            "",
            theme.text,
            Msg::OpenProjectSettings(id.to_owned()),
        ));
    }
    items.extend(order::project_moves(state, theme, id));
    items.push(separator(theme));
    items.push(item(
        theme,
        "Delete project",
        "threads are kept",
        theme.danger,
        Msg::DeleteProject(id.to_owned()),
    ));
    items
}
