//! The new-thread dialog's choice for a board task: a new thread, or a new tab in one
//! that exists (ADR 0018). In Existing mode it lists the threads to pick from.

use std::collections::HashMap;

use urushi_ui::widgets::{ButtonKind, SelectOption, button, modal};
use urushi_ui::{Key, Node, Theme};

use super::dialogs::{cancel, hint, label};
use super::new_thread::options_list;
use crate::msg::Msg;
use crate::state::AppState;

/// The "New thread | Existing thread" switch.
pub(super) fn switch(theme: &Theme, existing: bool) -> Node<Msg> {
    let kind = |on: bool| {
        if on {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        }
    };
    Node::row()
        .gap(8.0)
        .child(button(
            theme,
            "New thread",
            kind(!existing),
            Msg::UseExistingThread(false),
        ))
        .child(button(
            theme,
            "Existing thread",
            kind(existing),
            Msg::UseExistingThread(true),
        ))
}

/// The dialog in Existing mode: `body` (the task and the switch so far), then the
/// threads to pick from.
pub(super) fn dialog(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    body: Node<Msg>,
    picked: Option<&str>,
) -> Node<Msg> {
    modal(
        theme,
        "Start in a thread",
        480.0,
        body.children(threads(state, theme, scroll, picked)),
        vec![
            cancel(theme),
            button(theme, "Start in thread", ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}

/// The threads to pick from, and what starting there does.
fn threads(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    picked: Option<&str>,
) -> Vec<Node<Msg>> {
    let choices = state.existing_thread_choices();
    let mut out = vec![label(theme, "Thread")];
    if choices.is_empty() {
        out.push(hint(theme, "No threads yet: start a new one."));
        return out;
    }
    let options = choices
        .into_iter()
        .map(|(id, text)| SelectOption {
            label: text,
            selected: picked == Some(id.as_str()),
            on_pick: Msg::PickExistingThread(id),
        })
        .collect();
    out.push(options_list(theme, scroll, "existing-thread", options));
    out.push(hint(theme, &what_starts(state, picked.is_some())));
    out
}

/// What "Start in thread" will do.
fn what_starts(state: &AppState, picked: bool) -> String {
    if !picked {
        return "Pick the thread to start this task in.".to_owned();
    }
    match state.task_agent() {
        Some((agent, mode)) => format!(
            "A new tab there starts {} with this task as its prompt.",
            crate::task_agent::describe(agent, mode)
        ),
        None => "No agent is set for tasks (Settings): a terminal tab opens there.".to_owned(),
    }
}
