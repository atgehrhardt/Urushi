//! What Jira boards add to the panel (protocol 1.12): a badge on their tab, their sync
//! state and actions (Sync now, Open in Jira, Remove) instead of Rename and Delete, the
//! epic filter, and on cards a kind marker, the issue key, the epic chip and the
//! assignee's initials.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::super::labels;
use super::{TAB_CHARS, shorten};
use crate::msg::{JiraMsg, Msg};
use crate::organize::MenuTarget;
use crate::state::AppState;

/// Longest epic name shown whole in a card's chip.
const EPIC_CHARS: usize = 16;
/// Longest sync error shown whole in the header.
const ERROR_CHARS: usize = 40;

/// The small "J" before a Jira board's name on its tab; a red one when its sync failed.
pub(super) fn tab_badge(theme: &Theme, board: &v1::Board) -> Option<Node<Msg>> {
    let jira = board.jira.as_deref()?;
    let c = if jira.error.is_empty() {
        labels::color(theme, v1::LabelColor::Blue.into()).unwrap_or(theme.accent)
    } else {
        theme.danger
    };
    Some(
        Node::row()
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0))
            .radius(3.0)
            .justify(Align::Center)
            .align(Align::Center)
            .background(c.with_alpha(0.25))
            .role(
                Role::Label,
                if jira.error.is_empty() {
                    "Jira board"
                } else {
                    "Jira board, not syncing"
                },
            )
            .child(Node::text("J", Font::UiSemibold, 9.0, c)),
    )
}

/// "Synced 2 min ago", "Syncing…" or why it isn't syncing.
pub(crate) fn sync_label(jira: &v1::JiraBoardSource, now_ms: i64) -> String {
    if !jira.error.is_empty() {
        return jira.error.clone();
    }
    if jira.synced_at_ms == 0 {
        return "Syncing…".into();
    }
    let seconds = (now_ms - jira.synced_at_ms).max(0) / 1000;
    let ago = match seconds {
        _ if now_ms == 0 => return "Synced".into(),
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    };
    let dropped = if jira.dropped > 0 {
        " · some issues not shown"
    } else {
        ""
    };
    format!("Synced {ago}{dropped}")
}

/// A Jira board's header actions: its sync state, Sync now, Open in Jira and Remove.
pub(super) fn actions(state: &AppState, theme: &Theme, board: &v1::Board) -> Vec<Node<Msg>> {
    let Some(jira) = board.jira.as_deref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if !state.compact {
        let failed = !jira.error.is_empty();
        out.push(
            Node::text(
                shorten(&sync_label(jira, state.now_ms), ERROR_CHARS),
                Font::Ui,
                theme.font_size_small,
                if failed {
                    theme.danger
                } else {
                    theme.text_muted
                },
            )
            .padding(Insets::symmetric(0.0, 6.0)),
        );
    }
    let signed_in = state.jira.as_ref().is_some_and(|a| a.connected);
    out.push(if signed_in {
        button(
            theme,
            "Sync now",
            ButtonKind::Ghost,
            Msg::Jira(JiraMsg::Sync(board.id.clone())),
        )
    } else {
        button(
            theme,
            "Connect…",
            ButtonKind::Ghost,
            Msg::Jira(JiraMsg::OpenConnect),
        )
    });
    if !state.compact {
        out.push(button(
            theme,
            "Open in Jira",
            ButtonKind::Ghost,
            Msg::Jira(JiraMsg::Open(jira.url.clone())),
        ));
    }
    out.push(button(
        theme,
        "Remove",
        ButtonKind::Ghost,
        Msg::OpenDeleteBoard(board.id.clone()),
    ));
    out
}

/// The epic filter's button: "All epics" or the epic shown. Only on boards whose cards
/// are in epics.
pub(super) fn epic_filter(state: &AppState, theme: &Theme, board: &v1::Board) -> Option<Node<Msg>> {
    if crate::board::board_epics(board).is_empty() {
        return None;
    }
    let shown = state
        .board
        .epic_filter
        .as_deref()
        .and_then(|k| board.epics.iter().find(|e| e.key == k));
    let (text, color) = match shown {
        Some(e) => (
            format!("{}  ▾", shorten(&e.summary, TAB_CHARS)),
            labels::color(theme, e.color).unwrap_or(theme.text),
        ),
        None => ("All epics  ▾".to_owned(), theme.text_muted),
    };
    Some(
        Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .radius(theme.radius)
            .border(1.0, theme.border)
            .hover_background(theme.hover)
            .key(key(&["board-epic-filter"]))
            .on_click(Msg::OpenMenu(MenuTarget::EpicFilter(board.id.clone())))
            .role(Role::Button, "Filter by epic")
            .child(Node::text(
                text,
                Font::UiMedium,
                theme.font_size_small,
                color,
            )),
    )
}

/// The color a kind is drawn in.
pub(in crate::view) fn kind_color(theme: &Theme, kind: i32) -> Option<Color> {
    let wire = match v1::TaskKind::try_from(kind).ok()? {
        v1::TaskKind::Unspecified => return None,
        v1::TaskKind::Task => v1::LabelColor::Blue,
        v1::TaskKind::Story => v1::LabelColor::Green,
        v1::TaskKind::Bug => v1::LabelColor::Red,
        v1::TaskKind::Epic => v1::LabelColor::Purple,
        v1::TaskKind::Subtask => v1::LabelColor::Teal,
    };
    labels::color(theme, wire.into())
}

/// The kind's name, capitalized ("Story"), or `None` without one.
pub(in crate::view) fn kind_name(kind: i32) -> Option<&'static str> {
    Some(match v1::TaskKind::try_from(kind).ok()? {
        v1::TaskKind::Unspecified => return None,
        v1::TaskKind::Task => "Task",
        v1::TaskKind::Story => "Story",
        v1::TaskKind::Bug => "Bug",
        v1::TaskKind::Epic => "Epic",
        v1::TaskKind::Subtask => "Subtask",
    })
}

/// A small square in the kind's color (round for a bug), before a card's title.
pub(in crate::view) fn kind_marker(theme: &Theme, kind: i32, size: f32) -> Option<Node<Msg>> {
    let c = kind_color(theme, kind)?;
    let round = kind == i32::from(v1::TaskKind::Bug);
    Some(
        Node::new()
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .radius(if round { size / 2.0 } else { 2.0 })
            .background(c)
            .role(Role::Label, kind_name(kind).unwrap_or_default()),
    )
}

/// The issue key before a card's title, e.g. "ACME-12".
pub(super) fn key_label(theme: &Theme, issue: &v1::JiraIssue, color: Color) -> Node<Msg> {
    Node::text(&issue.key, Font::UiMedium, theme.font_size_small, color)
}

/// The card's epic, as a chip in the epic's color.
pub(super) fn epic_chip(theme: &Theme, board: &v1::Board, task: &v1::Task) -> Option<Node<Msg>> {
    let issue = task.issue.as_deref()?;
    let epic = board.epics.iter().find(|e| e.key == issue.epic_key)?;
    let c = labels::color(theme, epic.color).unwrap_or(theme.text_muted);
    let size = theme.font_size_small - 1.0;
    Some(
        Node::row()
            .padding(Insets::symmetric(1.0, 6.0))
            .radius(4.0)
            .border(1.0, c.with_alpha(0.6))
            .role(Role::Item, format!("Epic {}", epic.summary))
            .child(Node::text(
                shorten(&epic.summary, EPIC_CHARS),
                Font::UiMedium,
                size,
                c,
            )),
    )
}

/// "AL" for "Ada Lovelace".
pub(crate) fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let first = |w: &str| {
        w.chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
    };
    match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect::<String>().to_uppercase(),
        [a, .., b] => format!(
            "{}{}",
            first(a).unwrap_or_default(),
            first(b).unwrap_or_default()
        ),
    }
}

/// The assignee's initials in a circle.
pub(super) fn assignee(theme: &Theme, task: &v1::Task) -> Option<Node<Msg>> {
    let issue = task.issue.as_deref()?;
    if issue.assignee.is_empty() {
        return None;
    }
    let side = 16.0;
    Some(
        Node::row()
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .radius(side / 2.0)
            .justify(Align::Center)
            .align(Align::Center)
            .background(theme.selected)
            .border(1.0, theme.border)
            .role(Role::Label, format!("Assigned to {}", issue.assignee))
            .child(Node::text(
                initials(&issue.assignee),
                Font::UiSemibold,
                8.0,
                theme.text,
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_and_sync_labels() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("Grace Brewster Hopper"), "GH");
        assert_eq!(initials("ada"), "AD");
        assert_eq!(initials(""), "");
        let jira = |synced, error: &str| v1::JiraBoardSource {
            synced_at_ms: synced,
            error: error.into(),
            ..Default::default()
        };
        assert_eq!(sync_label(&jira(0, ""), 1_000), "Syncing…");
        assert_eq!(sync_label(&jira(1_000, ""), 0), "Synced");
        assert_eq!(sync_label(&jira(1_000, ""), 31_000), "Synced just now");
        assert_eq!(sync_label(&jira(1_000, ""), 181_000), "Synced 3 min ago");
        assert_eq!(sync_label(&jira(1_000, "offline"), 181_000), "offline");
    }
}
