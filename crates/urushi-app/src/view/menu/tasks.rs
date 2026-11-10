//! The board's menus: a task's, card or subtask (open, move to a column or make it
//! standalone, make it a subtask of another, color, start or go to its thread, delete
//! without opening it), a column's (rename, color, move, remove), and the
//! tag filter.

use urushi_ui::{Font, Insets, Key, Length, Node, Role, Theme, key};

use super::super::labels::{color, color_name, dot, palette};
use super::{back, check, item, separator};
use crate::msg::{BoardMsg, Msg};
use crate::organize::{ContextMenu, MenuPage, Prompt};
use crate::state::{AppState, TaskLink};

fn act(m: BoardMsg) -> Msg {
    Msg::Board(m)
}

pub(super) fn task_items(
    state: &AppState,
    theme: &Theme,
    menu: &ContextMenu,
    id: &str,
) -> Vec<Node<Msg>> {
    let Some(found) = state.workspace.find_task(id) else {
        return Vec::new();
    };
    let task = found.task();
    let id = task.id().to_owned();
    // Jira owns nesting and deleting its issues.
    let issue = task.issue();
    if let Some(items) = task_page(theme, menu, &found) {
        return items;
    }
    let extras = state.daemon_supports_board_extras();
    let mut items = vec![item(
        theme,
        "Open",
        "",
        theme.text,
        act(BoardMsg::OpenTask(id.clone())),
    )];
    if task.is_card() {
        items.push(item(
            theme,
            "Move to",
            &format!("{}  ›", found.column.name),
            theme.text,
            Msg::MenuPage(MenuPage::Move),
        ));
    } else if issue.is_none() {
        items.push(item(
            theme,
            "Make standalone",
            "",
            theme.text,
            act(BoardMsg::MakeStandalone(id.clone())),
        ));
    }
    if extras {
        if issue.is_none() {
            items.push(item(
                theme,
                "Make subtask of…",
                "",
                theme.text,
                act(BoardMsg::OpenNest(id.clone())),
            ));
        }
        items.push(item(
            theme,
            "Color",
            &format!("{}  ›", color_name(task.color())),
            theme.text,
            Msg::MenuPage(MenuPage::Color),
        ));
    }
    if state.daemon_supports_subtasks() {
        let link = TaskLink::of(&found);
        let label = if state.linked_thread(&link).is_some() {
            "Go to thread"
        } else {
            "Start thread…"
        };
        items.push(item(theme, label, "", theme.text, Msg::StartThread(link)));
    }
    if let Some(issue) = issue {
        items.push(item(
            theme,
            "Open in Jira",
            &issue.key,
            theme.text,
            Msg::Jira(crate::msg::JiraMsg::Open(issue.url.clone())),
        ));
        return items;
    }
    items.push(separator(theme));
    items.push(item(
        theme,
        if task.is_card() {
            "Delete task"
        } else {
            "Delete subtask"
        },
        "",
        theme.danger,
        act(BoardMsg::DeleteTask(id)),
    ));
    items
}

/// A task menu's sub-page (move to a column, color), if one is open.
fn task_page(
    theme: &Theme,
    menu: &ContextMenu,
    found: &urushi_client::FoundTask<'_>,
) -> Option<Vec<Node<Msg>>> {
    let task = found.task();
    let id = task.id().to_owned();
    match menu.page {
        MenuPage::Move => {
            let mut items = vec![back(theme, "Move to")];
            items.extend(found.board.columns.iter().map(|c| {
                item(
                    theme,
                    &c.name,
                    check(task.is_card() && c.id == found.column.id),
                    theme.text,
                    act(BoardMsg::MoveTask {
                        task: id.clone(),
                        column: c.id.clone(),
                    }),
                )
            }));
            Some(items)
        }
        MenuPage::Color => Some(color_page(theme, task.color(), |c| BoardMsg::ColorTask {
            task: id.clone(),
            color: c,
        })),
        MenuPage::Main | MenuPage::Status | MenuPage::Filter(_) => None,
    }
}

pub(super) fn column_items(
    state: &AppState,
    theme: &Theme,
    menu: &ContextMenu,
    id: &str,
) -> Vec<Node<Msg>> {
    let Some((board, column)) = state.workspace.column(id) else {
        return Vec::new();
    };
    if menu.page == MenuPage::Color {
        return color_page(theme, column.color, |c| BoardMsg::ColorColumn {
            column: column.id.clone(),
            color: c,
        });
    }
    let color_item = item(
        theme,
        "Color",
        &format!("{}  ›", color_name(column.color)),
        theme.text,
        Msg::MenuPage(MenuPage::Color),
    );
    // Jira owns a Jira board's columns; only their color is Urushi's.
    if board.jira.is_some() {
        return vec![color_item];
    }
    let at = board.columns.iter().position(|c| c.id == id).unwrap_or(0);
    let moved = |index: usize| {
        act(BoardMsg::MoveColumn {
            column: column.id.clone(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
        })
    };
    let mut items = vec![
        item(
            theme,
            "Rename…",
            "",
            theme.text,
            Msg::OpenPrompt(Prompt::RenameColumn(column.id.clone())),
        ),
        color_item,
    ];
    if at > 0 {
        items.push(item(theme, "Move left", "", theme.text, moved(at - 1)));
    }
    if at + 1 < board.columns.len() {
        items.push(item(theme, "Move right", "", theme.text, moved(at + 1)));
    }
    items.push(item(
        theme,
        "Add column…",
        "",
        theme.text,
        Msg::OpenPrompt(Prompt::NewColumn(board.id.clone())),
    ));
    if board.columns.len() > 1 {
        items.push(separator(theme));
        items.push(item(
            theme,
            "Remove column…",
            "",
            theme.danger,
            act(BoardMsg::OpenDeleteColumn(column.id.clone())),
        ));
    }
    items
}

/// "None", then each palette color with its swatch.
fn color_page(theme: &Theme, current: i32, pick: impl Fn(i32) -> BoardMsg) -> Vec<Node<Msg>> {
    let mut items = vec![back(theme, "Color")];
    items.push(dot_item(
        theme,
        ("None", None),
        current == 0,
        key(&["menu-color", "none"]),
        act(pick(0)),
    ));
    items.extend(palette().map(|(_, wire)| {
        let name = color_name(wire);
        dot_item(
            theme,
            (&name, color(theme, wire)),
            current == wire,
            key(&["menu-color", &name]),
            act(pick(wire)),
        )
    }));
    items
}

/// A menu row with a color dot before its label, and a check mark when chosen.
pub(super) fn dot_item(
    theme: &Theme,
    (label, fill): (&str, Option<urushi_ui::Color>),
    chosen: bool,
    key: Key,
    msg: Msg,
) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(28.0))
        .padding(Insets::symmetric(0.0, 10.0))
        .gap(8.0)
        .radius(theme.radius)
        .hover_background(theme.hover)
        .key(key)
        .on_click(msg)
        .role(Role::Button, label)
        .child(dot(theme, fill, 10.0))
        .child(Node::text(label, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)))
        .child(Node::text(
            check(chosen),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
}
