//! Confirmations on the board: deleting a board (its tasks go with it) and removing a
//! column (its tasks move to the column next to it). Only the red button acts; Enter
//! doesn't.

use urushi_ui::widgets::{ButtonKind, button, modal};
use urushi_ui::{Font, Length, Node, Theme};

use super::super::dialogs::{cancel, hint};
use crate::msg::{BoardMsg, Msg};

/// Deleting a board takes its tasks with it, so it asks first. Only the red button
/// deletes (Enter doesn't). A Jira board is only removed here: Jira keeps its issues.
pub(in crate::view) fn delete_board(
    theme: &Theme,
    name: &str,
    tasks: usize,
    jira: bool,
) -> Node<Msg> {
    let (title, what, after, action) = if jira {
        (
            format!("Remove board “{name}”?"),
            "It stops following Jira here. Nothing changes in Jira.".to_owned(),
            "Threads started from its issues stay. You can import it again.",
            "Remove board",
        )
    } else {
        let what = match tasks {
            0 => "It has no tasks.".to_owned(),
            1 => "Its 1 task will be deleted too.".to_owned(),
            n => format!("Its {n} tasks will be deleted too."),
        };
        (
            format!("Delete board “{name}”?"),
            what,
            "This can't be undone.",
            "Delete board",
        )
    };
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(Node::text(what, Font::Ui, theme.font_size, theme.text))
        .child(hint(theme, after));
    modal(
        theme,
        &title,
        420.0,
        body,
        vec![
            cancel(theme),
            button(theme, action, ButtonKind::Danger, Msg::ConfirmDeleteBoard),
        ],
        Some(Msg::CloseDialog),
    )
}

/// Removing a column moves its tasks next door, so it says where they go.
pub(in crate::view) fn delete_column(
    theme: &Theme,
    name: &str,
    tasks: usize,
    into: &str,
) -> Node<Msg> {
    let what = match tasks {
        0 => "It has no tasks.".to_owned(),
        1 => format!("Its 1 task will move to “{into}”."),
        n => format!("Its {n} tasks will move to “{into}”."),
    };
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(Node::text(what, Font::Ui, theme.font_size, theme.text));
    modal(
        theme,
        &format!("Remove column “{name}”?"),
        420.0,
        body,
        vec![
            cancel(theme),
            button(
                theme,
                "Remove column",
                ButtonKind::Danger,
                Msg::Board(BoardMsg::ConfirmDeleteColumn),
            ),
        ],
        Some(Msg::CloseDialog),
    )
}
