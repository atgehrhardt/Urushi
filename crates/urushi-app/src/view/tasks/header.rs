//! The task editor's top bar, as Jira heads an issue: **‹** back to the task the
//! editor was opened from, then where the task is (its epic on a Jira board, and the
//! tasks it is part of, each opening that task), then the task itself.

use urushi_client::{FoundTask, TaskRef};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::super::board::jira::kind_marker;
use crate::msg::{BoardMsg, JiraMsg, Msg};
use crate::state::{AppState, EditorNav};

/// Longest task title shown whole in a crumb.
const CRUMB_CHARS: usize = 28;
/// Longest title in a crumb on a phone.
const CRUMB_CHARS_NARROW: usize = 14;

/// The header and the dialog's name (for accessibility).
pub(super) fn header(
    state: &AppState,
    theme: &Theme,
    found: Option<&FoundTask<'_>>,
    nav: &EditorNav,
) -> (Node<Msg>, String) {
    let current = found.map(FoundTask::task);
    let label = match current {
        None if nav.draft_of_previous.is_some() => "New subtask".to_owned(),
        None => "New task".to_owned(),
        Some(c) if c.is_card() => "Edit task".to_owned(),
        Some(_) => "Edit subtask".to_owned(),
    };
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .gap(4.0)
        .align(Align::Center)
        .clip();
    if !nav.back.is_empty() {
        row = row.child(back_button(theme));
    }
    if let Some(found) = found {
        let chars = if state.compact {
            CRUMB_CHARS_NARROW
        } else {
            CRUMB_CHARS
        };
        if let Some(epic) = epic_crumb(theme, found, chars) {
            row = row.child(epic).child(separator(theme));
        }
        let (current, ancestors) = found
            .path
            .split_last()
            .map_or((None, &[][..]), |(c, a)| (Some(*c), a));
        // On a phone only the parent shows (‹ PARENT-1 / …), the rest fold away.
        let shown = if state.compact && ancestors.len() > 1 {
            row = row.child(muted(theme, "…")).child(separator(theme));
            &ancestors[ancestors.len() - 1..]
        } else {
            ancestors
        };
        for t in shown {
            row = row.child(crumb(theme, *t, chars)).child(separator(theme));
        }
        if let Some(c) = current {
            row = row.child(here(theme, c, &label));
        }
    } else {
        row = row.child(title(theme, &label, theme.text));
    }
    (row, label)
}

/// **‹ Back**: the task the editor was opened from.
fn back_button(theme: &Theme) -> Node<Msg> {
    Node::row()
        .padding(Insets::symmetric(2.0, 6.0))
        .radius(6.0)
        .hover_background(theme.hover)
        .key(key(&["editor-back"]))
        .on_click(Msg::Board(BoardMsg::EditorBack))
        .role(Role::Button, "Back")
        .child(Node::text(
            "‹",
            Font::UiSemibold,
            theme.font_size + 4.0,
            theme.text,
        ))
}

/// The card's epic on a Jira board (subtasks name theirs through their card); it isn't
/// a task, so it opens in Jira.
fn epic_crumb(theme: &Theme, found: &FoundTask<'_>, chars: usize) -> Option<Node<Msg>> {
    let issue = found.card.issue.as_deref()?;
    let epic = found.board.epics.iter().find(|e| e.key == issue.epic_key)?;
    let c = super::super::labels::color(theme, epic.color).unwrap_or(theme.text_muted);
    let mut node = Node::row()
        .gap(5.0)
        .align(Align::Center)
        .padding(Insets::symmetric(1.0, 4.0))
        .radius(4.0)
        .role(Role::Button, format!("Open epic {} in Jira", epic.summary))
        .child(
            Node::new()
                .width(Length::Fixed(8.0))
                .height(Length::Fixed(8.0))
                .radius(2.0)
                .background(c),
        )
        .child(Node::text(
            super::super::board::shorten(&epic.summary, chars),
            Font::UiMedium,
            theme.font_size_small,
            c,
        ));
    if !epic.url.is_empty() {
        node = node
            .hover_background(theme.hover)
            .key(key(&["crumb-epic", &epic.key]))
            .on_click(Msg::Jira(JiraMsg::Open(epic.url.clone())));
    }
    Some(node)
}

/// A task the one on screen is part of: its kind and key (a Jira issue) or its title.
fn crumb(theme: &Theme, t: TaskRef<'_>, chars: usize) -> Node<Msg> {
    let text = t.issue().map_or_else(
        || super::super::board::shorten(t.title(), chars),
        |i| i.key.clone(),
    );
    let mut node = Node::row()
        .gap(5.0)
        .align(Align::Center)
        .padding(Insets::symmetric(1.0, 4.0))
        .radius(4.0)
        .hover_background(theme.hover)
        .key(key(&["breadcrumb", t.id()]))
        .on_click(Msg::Board(BoardMsg::OpenTask(t.id().to_owned())))
        .role(Role::Button, format!("Open {}", t.title()));
    if let Some(marker) = kind_marker(theme, t.kind(), 9.0) {
        node = node.child(marker);
    }
    node.child(Node::text(
        text,
        Font::UiMedium,
        theme.font_size_small,
        theme.accent,
    ))
}

/// The task on screen: its kind and key for a Jira issue, otherwise what the editor
/// does ("Edit subtask").
fn here(theme: &Theme, t: TaskRef<'_>, label: &str) -> Node<Msg> {
    let Some(issue) = t.issue() else {
        return title(theme, label, theme.text);
    };
    let mut node = Node::row().gap(6.0).align(Align::Center);
    if let Some(marker) = kind_marker(theme, t.kind(), 11.0) {
        node = node.child(marker);
    }
    node.child(title(theme, &issue.key, theme.text))
}

fn title(theme: &Theme, text: &str, color: Color) -> Node<Msg> {
    Node::text(text, Font::UiSemibold, theme.font_size + 2.0, color)
}

fn muted(theme: &Theme, text: &str) -> Node<Msg> {
    Node::text(text, Font::Ui, theme.font_size_small, theme.text_muted)
}

fn separator(theme: &Theme) -> Node<Msg> {
    muted(theme, "/")
}
