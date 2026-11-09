//! The board's menus: a task card's (open, move to a column, color, start or go to its
//! thread, delete without opening it), a column's (rename, color, move, remove), and the
//! tag filter.

use urushi_ui::{Font, Insets, Key, Length, Node, Role, Theme, key};

use super::super::labels::{color, color_name, dot, palette, tag_color};
use super::{back, check, item, separator};
use crate::board::board_tags;
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
    let Some((board, column, task)) = state.workspace.task(id) else {
        return Vec::new();
    };
    match menu.page {
        MenuPage::Move => {
            let mut items = vec![back(theme, "Move to")];
            items.extend(board.columns.iter().map(|c| {
                item(
                    theme,
                    &c.name,
                    check(c.id == column.id),
                    theme.text,
                    act(BoardMsg::MoveTask {
                        task: task.id.clone(),
                        column: c.id.clone(),
                    }),
                )
            }));
            return items;
        }
        MenuPage::Color => {
            return color_page(theme, task.color, |c| BoardMsg::ColorTask {
                task: task.id.clone(),
                color: c,
            });
        }
        MenuPage::Main | MenuPage::Status => {}
    }
    let mut items = vec![
        item(
            theme,
            "Open",
            "",
            theme.text,
            act(BoardMsg::OpenTask(task.id.clone())),
        ),
        item(
            theme,
            "Move to",
            &format!("{}  ›", column.name),
            theme.text,
            Msg::MenuPage(MenuPage::Move),
        ),
    ];
    if state.daemon_supports_board_extras() {
        items.push(item(
            theme,
            "Color",
            &format!("{}  ›", color_name(task.color)),
            theme.text,
            Msg::MenuPage(MenuPage::Color),
        ));
    }
    if state.daemon_supports_subtasks() {
        let link = TaskLink {
            task: task.id.clone(),
            subtask: None,
        };
        let label = if state.linked_thread(&link).is_some() {
            "Go to thread"
        } else {
            "Start thread…"
        };
        items.push(item(theme, label, "", theme.text, Msg::StartThread(link)));
    }
    items.push(separator(theme));
    items.push(item(
        theme,
        "Delete task",
        "",
        theme.danger,
        act(BoardMsg::DeleteTask(task.id.clone())),
    ));
    items
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
        item(
            theme,
            "Color",
            &format!("{}  ›", color_name(column.color)),
            theme.text,
            Msg::MenuPage(MenuPage::Color),
        ),
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

/// "All tasks", then one row per tag used on the board.
pub(super) fn tag_filter_items(state: &AppState, theme: &Theme, board: &str) -> Vec<Node<Msg>> {
    let Some(board) = state.workspace.board(board) else {
        return Vec::new();
    };
    let current = state.board.tag_filter.as_deref();
    let mut items = vec![item(
        theme,
        "All tasks",
        check(current.is_none()),
        theme.text,
        act(BoardMsg::FilterTag(None)),
    )];
    items.extend(board_tags(board).into_iter().map(|tag| {
        let on = current.is_some_and(|c| c.eq_ignore_ascii_case(&tag));
        dot_item(
            theme,
            (&tag, Some(tag_color(theme, &tag))),
            on,
            key(&["menu-tag", &tag]),
            act(BoardMsg::FilterTag(Some(tag.clone()))),
        )
    }));
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
fn dot_item(
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
