//! The task editor's subtasks: an existing task's (checked off in place, each change
//! a request, and each opening as a task of its own) or the drafts of a new one, and
//! the field that adds one.

use urushi_client::{TaskRef, v1};
use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::super::dialogs::{field, label};
use super::subtasks_scroll_key;
use crate::msg::{BoardMsg, Msg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Field, SubtaskEditor, TaskLink};

/// Subtask rows shown before the list scrolls.
const VISIBLE_SUBTASKS: usize = 6;
/// Height of one subtask row.
const SUBTASK_ROW: f32 = 28.0;

/// The "Subtasks" heading, the list (an existing task's subtasks, each opening as a
/// task of its own; or drafts for a new task), and the field that adds one.
pub(super) fn checklist(
    state: &AppState,
    theme: &Theme,
    task: Option<TaskRef<'_>>,
    editor: &SubtaskEditor,
    offset: f32,
) -> Vec<Node<Msg>> {
    let mut rows: Vec<Node<Msg>> = Vec::new();
    let heading = match task {
        Some(t) if !t.subtasks().is_empty() => {
            let (done, all) = t.progress();
            format!("Subtasks  {done}/{all}")
        }
        _ => "Subtasks".to_owned(),
    };
    match task {
        Some(t) => {
            for s in t.subtasks() {
                rows.push(subtask_row(state, theme, s));
            }
        }
        None => {
            for (i, title) in editor.drafts.iter().enumerate() {
                rows.push(draft_row(theme, i, title));
            }
        }
    }
    let mut out = vec![label(theme, &heading)];
    if !rows.is_empty() {
        let mut list = Node::column().width(Length::Fill(1.0)).gap(0.0);
        if rows.len() > VISIBLE_SUBTASKS {
            #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
            let height = VISIBLE_SUBTASKS as f32 * SUBTASK_ROW;
            list = list
                .height(Length::Fixed(height))
                .scroll(subtasks_scroll_key(), offset);
        }
        out.push(list.children(rows));
    }
    out.push(field(
        state,
        theme,
        &editor.new,
        Field::NewSubtask,
        "Add a subtask…",
    ));
    out
}

/// One subtask: its check box, its title (which opens it, on a daemon that keeps
/// subtasks as tasks), its color, its own progress, its thread, and ×. A right-click
/// opens its menu.
fn subtask_row(state: &AppState, theme: &Theme, s: &v1::Subtask) -> Node<Msg> {
    let link = state.workspace.find_task(&s.id).map(|f| TaskLink::of(&f));
    let started = link
        .as_ref()
        .is_some_and(|l| state.linked_thread(l).is_some());
    let openable = state.daemon_supports_board_extras();
    let mut title = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center);
    if let Some(c) = super::super::labels::color(theme, s.color) {
        title = title.child(super::super::labels::dot(theme, Some(c), 8.0));
    }
    title = title.child(
        Node::text(
            &s.title,
            Font::Ui,
            theme.font_size,
            if s.done { theme.text_muted } else { theme.text },
        )
        // Fill, so a long title is cut short instead of running over the row.
        .width(Length::Fill(1.0)),
    );
    let sub = TaskRef::Sub(s);
    if !s.subtasks.is_empty() {
        let (done, all) = sub.progress();
        title = title.child(Node::text(
            format!("✓ {done}/{all}"),
            Font::UiMedium,
            theme.font_size_small,
            theme.text_muted,
        ));
    }
    if openable {
        title = title
            .key(key(&["subtask-title", &s.id]))
            .on_click(Msg::Board(BoardMsg::OpenTask(s.id.clone())))
            .role(Role::Button, format!("Open subtask {}", s.title));
    }
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(SUBTASK_ROW))
        .padding(Insets::symmetric(0.0, 4.0))
        .gap(8.0)
        .radius(theme.radius)
        .hover_background(theme.hover)
        .child(checkbox(
            theme,
            s.done,
            &s.title,
            Msg::ToggleSubtask(s.id.clone()),
        ))
        .child(title);
    if openable {
        row = row.on_secondary_click(Msg::OpenMenu(MenuTarget::Task(s.id.clone())));
    }
    if let Some(link) = link {
        let (text, color, what) = if started {
            ("Go to thread", theme.accent, "Go to thread")
        } else {
            ("Start thread", theme.text_muted, "Start a thread")
        };
        row = row.child(small_action(
            theme,
            text,
            color,
            &format!("{what} for {}", s.title),
            Msg::StartThread(link),
        ));
    }
    row.child(icon_button(
        theme,
        "×",
        &format!("Delete subtask {}", s.title),
        Msg::DeleteSubtask(s.id.clone()),
    ))
}

fn draft_row(theme: &Theme, i: usize, title: &str) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(SUBTASK_ROW))
        .padding(Insets::symmetric(0.0, 4.0))
        .gap(8.0)
        .child(checkbox_shape(theme, false))
        .child(Node::text(title, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)))
        .child(icon_button(
            theme,
            "×",
            &format!("Remove subtask {title}"),
            Msg::RemoveDraftSubtask(i),
        ))
}

/// A check box that sends `on_click`.
fn checkbox(theme: &Theme, on: bool, what: &str, on_click: Msg) -> Node<Msg> {
    checkbox_shape(theme, on)
        .key(key(&["check", what]))
        .on_click(on_click)
        .role(
            Role::Switch,
            format!("{} {what}", if on { "Uncheck" } else { "Check" }),
        )
}

pub(super) fn checkbox_shape(theme: &Theme, on: bool) -> Node<Msg> {
    let mut b = Node::row()
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .radius(4.0)
        .justify(Align::Center)
        .align(Align::Center)
        .border(1.5, if on { theme.accent } else { theme.text_muted })
        .background(if on { theme.accent } else { Color::TRANSPARENT });
    if on {
        b = b.child(Node::text(
            "✓",
            Font::UiSemibold,
            theme.font_size_small,
            theme.on_accent,
        ));
    }
    b
}

/// A small text button for a row (e.g. "Start thread").
fn small_action(theme: &Theme, text: &str, color: Color, label: &str, on_click: Msg) -> Node<Msg> {
    Node::row()
        .padding(Insets::symmetric(2.0, 6.0))
        .radius(4.0)
        .hover_background(theme.hover)
        .key(key(&["small-action", label]))
        .on_click(on_click)
        .role(Role::Button, label)
        .child(Node::text(
            text,
            Font::UiMedium,
            theme.font_size_small,
            color,
        ))
}
