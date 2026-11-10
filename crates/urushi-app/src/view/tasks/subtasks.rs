//! The task editor's subtasks: an existing task's (checked off in place, each check a
//! request, and each opening as a task of its own), the drafts added here (created on
//! Save, each opening in an editor of its own), the field that adds one by its title,
//! and **New subtask…**, which writes one with all its details.

use urushi_client::{TaskRef, v1};
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::super::dialogs::{field, label};
use super::subtasks_scroll_key;
use crate::drafts::SubtaskDraft;
use crate::msg::{BoardMsg, Msg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Field, SubtaskEditor, TaskLink};

/// Subtask rows shown before the list scrolls.
const VISIBLE_SUBTASKS: usize = 6;
/// Height of one subtask row.
const SUBTASK_ROW: f32 = 28.0;

/// The "Subtasks" heading, the list (an existing task's subtasks, each opening as a
/// task of its own; or drafts for a new task), and the field that adds one. A Jira
/// epic (`jira_epic`) can't have sub-tasks, so it says so instead of offering one.
pub(super) fn checklist(
    state: &AppState,
    theme: &Theme,
    (task, jira_epic): (Option<TaskRef<'_>>, bool),
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
    if let Some(t) = task {
        for s in t.subtasks() {
            rows.push(subtask_row(state, theme, s));
        }
    }
    // Added here, created on Save; each opens in an editor of its own (1.14).
    let openable = state.daemon_supports_subtask_details();
    for (i, draft) in editor.drafts.iter().enumerate() {
        rows.push(draft_row(theme, i, draft, openable));
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
    if jira_epic {
        out.push(
            Node::text(
                "Jira epics hold stories, not sub-tasks.",
                Font::Ui,
                theme.font_size_small,
                theme.text_muted,
            )
            .key(key(&["epic-no-subtasks"])),
        );
        return out;
    }
    let add = field(
        state,
        theme,
        &editor.new,
        Field::NewSubtask,
        "Add a subtask…",
    );
    if !openable {
        out.push(add);
        return out;
    }
    out.push(
        Node::row()
            .width(Length::Fill(1.0))
            .gap(8.0)
            .align(Align::Center)
            .child(add.width(Length::Fill(1.0)))
            .child(button(
                theme,
                "New subtask…",
                ButtonKind::Secondary,
                Msg::Board(BoardMsg::NewSubtask),
            )),
    );
    out
}

/// One subtask: its check box, its title (which opens it, on a daemon that keeps
/// subtasks as tasks), its color, its own progress, its thread, and × (not for a Jira
/// issue). A right-click opens its menu.
fn subtask_row(state: &AppState, theme: &Theme, s: &v1::Subtask) -> Node<Msg> {
    let link = state.workspace.find_task(&s.id).map(|f| TaskLink::of(&f));
    let started = link
        .as_ref()
        .is_some_and(|l| state.linked_thread(l).is_some());
    let openable = state.daemon_supports_board_extras();
    let title = subtask_title(theme, s, openable);
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
    if let Some(kind) = state.subtask_indicator(s) {
        row = row.child(super::super::attention::indicator(
            state,
            theme,
            kind,
            super::super::attention::DOT,
        ));
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
    // A Jira issue is deleted in Jira, after a question; older daemons can't.
    let on_jira = s.issue.is_some()
        || state
            .workspace
            .find_task(&s.id)
            .is_some_and(|f| f.board.jira.is_some());
    if on_jira && !state.daemon_supports_jira_deletion() {
        return row;
    }
    row.child(icon_button(
        theme,
        "×",
        &format!("Delete subtask {}", s.title),
        Msg::DeleteSubtask(s.id.clone()),
    ))
}

/// A subtask's title row: its color, kind and key (a Jira issue), title and progress;
/// it opens the subtask when `openable`.
fn subtask_title(theme: &Theme, s: &v1::Subtask, openable: bool) -> Node<Msg> {
    let mut title = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center);
    if let Some(c) = super::super::labels::color(theme, s.color) {
        title = title.child(super::super::labels::dot(theme, Some(c), 8.0));
    }
    if let Some(issue) = s.issue.as_deref() {
        if let Some(marker) = super::super::board::jira::kind_marker(theme, s.kind, 9.0) {
            title = title.child(marker);
        }
        title = title.child(Node::text(
            &issue.key,
            Font::UiMedium,
            theme.font_size_small,
            theme.text_muted,
        ));
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
    title
}

/// A subtask written here, created on Save: its title (opening its editor when
/// `openable`), whether it has more than a title, and × to remove it.
fn draft_row(theme: &Theme, i: usize, draft: &SubtaskDraft, openable: bool) -> Node<Msg> {
    let title = draft.title();
    let shown = if title.is_empty() {
        "Untitled subtask"
    } else {
        title
    };
    let mut text = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center)
        .child(Node::text(shown, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)));
    if draft.has_details() {
        text = text.child(Node::text(
            "details",
            Font::UiMedium,
            theme.font_size_small,
            theme.text_muted,
        ));
    }
    if openable {
        text = text
            .padding(Insets::symmetric(0.0, 4.0))
            .radius(theme.radius)
            .hover_background(theme.hover)
            .key(key(&["subtask-draft", &i.to_string()]))
            .on_click(Msg::Board(BoardMsg::OpenSubtaskDraft(i)))
            .role(Role::Button, format!("Open new subtask {shown}"));
    }
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(SUBTASK_ROW))
        .padding(Insets::symmetric(0.0, 4.0))
        .gap(8.0)
        .child(checkbox_shape(theme, false))
        .child(text)
        .child(icon_button(
            theme,
            "×",
            &format!("Remove subtask {shown}"),
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
