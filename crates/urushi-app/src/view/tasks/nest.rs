//! "Make subtask of…": a search field and the tasks of the board a task can go under,
//! each shown with where it is ("Card › Subtask"). Enter picks the first.

use std::collections::HashMap;

use urushi_ui::{Font, Insets, Key, Length, Node, Role, TextField, Theme, key};

use super::super::board::shorten;
use super::super::dialogs::{cancel, field, hint};
use crate::board::nest_candidates;
use crate::msg::{BoardMsg, Msg};
use crate::state::{AppState, Field};
use crate::view::modal::modal;

/// Rows shown before the list scrolls.
const VISIBLE: usize = 9;
/// Height of a row.
const ROW: f32 = 30.0;
/// Longest path shown whole.
const PATH_CHARS: usize = 60;

/// Key of the list's scroll.
pub(crate) fn nest_scroll_key() -> Key {
    key(&["nest-candidates"])
}

pub(in crate::view) fn picker(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    task: &str,
    query: &TextField,
) -> Node<Msg> {
    let title = state
        .workspace
        .find_task(task)
        .map_or_else(String::new, |f| f.task().title().to_owned());
    let candidates = nest_candidates(&state.workspace, task, query.text());
    let rows: Vec<Node<Msg>> = candidates
        .iter()
        .map(|(id, path)| {
            Node::row()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(ROW))
                .padding(Insets::symmetric(0.0, 8.0))
                .radius(theme.radius)
                .hover_background(theme.hover)
                .key(key(&["nest-under", id]))
                .on_click(Msg::Board(BoardMsg::Nest {
                    task: task.to_owned(),
                    parent: id.clone(),
                }))
                .role(Role::Button, format!("Put under {path}"))
                .child(
                    Node::text(
                        shorten(path, PATH_CHARS),
                        Font::Ui,
                        theme.font_size,
                        theme.text,
                    )
                    .width(Length::Fill(1.0)),
                )
        })
        .collect();
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(field(
            state,
            theme,
            query,
            Field::NestQuery,
            "Search tasks…",
        ));
    if rows.is_empty() {
        body = body.child(hint(theme, "No other task on this board matches."));
    } else {
        let mut list = Node::column().width(Length::Fill(1.0));
        if rows.len() > VISIBLE {
            #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
            let height = VISIBLE as f32 * ROW;
            let offset = scroll.get(&nest_scroll_key()).copied().unwrap_or(0.0);
            list = list
                .height(Length::Fixed(height))
                .scroll(nest_scroll_key(), offset);
        }
        body = body.child(list.children(rows));
    }
    modal(
        state,
        theme,
        &format!("Make “{}” a subtask of…", shorten(&title, 40)),
        480.0,
        body,
        vec![cancel(theme)],
        Some(Msg::CloseDialog),
    )
}
