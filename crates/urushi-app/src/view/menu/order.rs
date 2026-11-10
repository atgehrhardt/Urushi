//! "Move up" and "Move down" in the menus of projects and threads, where the item can go
//! that way and the daemon can reorder it.

use urushi_ui::{Node, Theme};

use super::item;
use crate::msg::Msg;
use crate::organize::{thread_drop_index, thread_neighbours};
use crate::state::AppState;

/// "Move up" and "Move down" for a project, where it can go that way and the daemon can
/// reorder projects (protocol 1.11).
pub(super) fn project_moves(state: &AppState, theme: &Theme, id: &str) -> Vec<Node<Msg>> {
    let projects = state.workspace.projects();
    let Some(at) = projects.iter().position(|p| p.id == id) else {
        return Vec::new();
    };
    if !state.daemon_supports_project_order() {
        return Vec::new();
    }
    let moved = |index: usize| Msg::MoveProject {
        project: id.to_owned(),
        index: u32::try_from(index).unwrap_or(u32::MAX),
    };
    let mut items = Vec::new();
    if at > 0 {
        items.push(item(theme, "Move up", "", theme.text, moved(at - 1)));
    }
    if at + 1 < projects.len() {
        items.push(item(theme, "Move down", "", theme.text, moved(at + 1)));
    }
    items
}

/// "Move up" and "Move down" for a thread, within its sidebar group (its project, pin
/// and status), where it has a neighbour that way and the daemon can reorder threads
/// (protocol 1.24). It takes the neighbour's place in the list of threads.
pub(super) fn thread_moves(state: &AppState, theme: &Theme, id: &str) -> Vec<Node<Msg>> {
    if !state.daemon_supports_thread_order() {
        return Vec::new();
    }
    let threads = &state.workspace.raw().threads;
    let (above, below) = thread_neighbours(&state.workspace, id);
    let moved = |to: Option<&urushi_client::v1::Thread>| {
        let index = thread_drop_index(threads, id, &to?.id)?;
        Some(Msg::ReorderThread {
            thread: id.to_owned(),
            index,
        })
    };
    [("Move up", moved(above)), ("Move down", moved(below))]
        .into_iter()
        .filter_map(|(label, msg)| Some(item(theme, label, "", theme.text, msg?)))
        .collect()
}
