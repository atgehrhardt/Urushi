//! The task editor's kind picker (any board) and, for a Jira issue, what Jira owns
//! shown read-only: its key, type, status, assignee, priority and epic, with a link to
//! the issue (protocol 1.12).

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::super::board::jira::{kind_color, kind_marker, kind_name};
use super::super::dialogs::label;
use crate::msg::{BoardMsg, JiraMsg, Msg};

/// The kinds a picker offers: on a Jira board only the issue types that are cards,
/// which includes epics when the daemon makes them cards (1.15, ADR 0027).
fn offered(jira: bool, epics: bool) -> Vec<v1::TaskKind> {
    let mut kinds = vec![v1::TaskKind::Task, v1::TaskKind::Story, v1::TaskKind::Bug];
    if !jira || epics {
        kinds.push(v1::TaskKind::Epic);
    }
    kinds
}

/// "Type" and a chip per kind; the chosen one again clears it (except on Jira boards,
/// where an issue needs a type).
pub(super) fn kind_picker(
    theme: &Theme,
    chosen: i32,
    (jira, epics): (bool, bool),
    width: f32,
) -> Vec<Node<Msg>> {
    let chips = offered(jira, epics).into_iter().map(|k| {
        let wire = i32::from(k);
        let on = wire == chosen || (jira && chosen == 0 && k == v1::TaskKind::Task);
        let name = kind_name(wire).unwrap_or_default();
        let next = if on && !jira { 0 } else { wire };
        let chip = Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .gap(6.0)
            .align(Align::Center)
            .radius(theme.radius)
            .border(
                1.0,
                if on {
                    kind_color(theme, wire).unwrap_or(theme.accent)
                } else {
                    theme.border
                },
            )
            .background(if on {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(theme.hover)
            .key(key(&["task-kind", name]))
            .on_click(Msg::Board(BoardMsg::PickKind(next)))
            .role(Role::Item, format!("Type {name}"))
            .children(kind_marker(theme, wire, 8.0))
            .child(Node::text(
                name,
                Font::UiMedium,
                theme.font_size_small,
                if on { theme.text } else { theme.text_muted },
            ));
        let size = theme.font_size_small;
        (super::super::labels::text_width(name, size) + 36.0, chip)
    });
    vec![
        label(theme, "Type"),
        super::super::labels::wrap(chips.collect(), 6.0, width),
    ]
}

/// A Jira issue's own fields, read-only, and a button to open it in Jira.
pub(super) fn issue_section(
    theme: &Theme,
    board: &v1::Board,
    issue: &v1::JiraIssue,
    kind: i32,
) -> Vec<Node<Msg>> {
    let row = |name: &str, value: String| {
        Node::row()
            .width(Length::Fill(1.0))
            .gap(8.0)
            .align(Align::Center)
            .child(
                Node::text(name, Font::Ui, theme.font_size_small, theme.text_muted)
                    .width(Length::Fixed(70.0)),
            )
            .child(
                Node::text(value, Font::UiMedium, theme.font_size_small, theme.text)
                    .width(Length::Fill(1.0)),
            )
    };
    let none = || "—".to_owned();
    let or_none = |s: &str| {
        if s.is_empty() { none() } else { s.to_owned() }
    };
    let epic = board
        .epics
        .iter()
        .find(|e| e.key == issue.epic_key)
        .map_or_else(none, |e| format!("{} ({})", e.summary, e.key));
    let kind_row = Node::row()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .align(Align::Center)
        .child(
            Node::text("Type", Font::Ui, theme.font_size_small, theme.text_muted)
                .width(Length::Fixed(70.0)),
        )
        .children(kind_marker(theme, kind, 8.0))
        .child(Node::text(
            or_none(&issue.type_name),
            Font::UiMedium,
            theme.font_size_small,
            theme.text,
        ));
    vec![
        label(theme, "Jira"),
        Node::row()
            .width(Length::Fill(1.0))
            .gap(8.0)
            .align(Align::Center)
            .child(
                Node::text(&issue.key, Font::UiSemibold, theme.font_size, theme.text)
                    .width(Length::Fill(1.0)),
            )
            .child(button(
                theme,
                "Open in Jira",
                ButtonKind::Ghost,
                Msg::Jira(JiraMsg::Open(issue.url.clone())),
            )),
        kind_row,
        row("Status", or_none(&issue.status)),
        row(
            "Assignee",
            if issue.assignee.is_empty() {
                "Unassigned".to_owned()
            } else {
                issue.assignee.clone()
            },
        ),
        row("Priority", or_none(&issue.priority)),
        row("Epic", epic),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_boards_offer_epics_only_from_daemons_that_make_them_cards() {
        let epic = |jira, epics| offered(jira, epics).contains(&v1::TaskKind::Epic);
        assert!(epic(false, false), "any board of Urushi's own");
        assert!(!epic(true, false), "an older daemon refuses them");
        assert!(epic(true, true));
    }
}
