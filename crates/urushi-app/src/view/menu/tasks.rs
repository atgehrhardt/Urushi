//! The context menu of a task card: open, move to another column, start or go to its
//! thread, and delete it without opening it.

use urushi_ui::{Node, Theme};

use super::{back, check, item, separator};
use crate::msg::{BoardMsg, Msg};
use crate::organize::{ContextMenu, MenuPage};
use crate::state::{AppState, TaskLink};

pub(super) fn task_items(
    state: &AppState,
    theme: &Theme,
    menu: &ContextMenu,
    id: &str,
) -> Vec<Node<Msg>> {
    let Some((board, column, task)) = state.workspace.task(id) else {
        return Vec::new();
    };
    let act = |m: BoardMsg| Msg::Board(m);
    if menu.page == MenuPage::Move {
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
