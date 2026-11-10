//! The quick-filter dialogs (ADR 0037): saving what is chosen on the board as a quick
//! filter, or changing one — a name, then on a Jira board its JQL (which Jira gets and
//! evaluates), elsewhere what it filters by — and asking before deleting one. Only the
//! red button deletes; Enter doesn't.

use urushi_ui::widgets::{ButtonKind, button, modal};
use urushi_ui::{Font, Length, Node, Theme};

use super::super::dialogs::{area, cancel, field, hint, label};
use crate::filter::editor::QuickFilterEditor;
use crate::filter::quick::{QuickRef, describe};
use crate::msg::{FilterMsg, Msg};
use crate::state::{AppState, Field};

/// Lines the JQL field shows.
const JQL_LINES: u8 = 4;

/// Saving or changing a quick filter.
pub(in crate::view) fn editor(state: &AppState, theme: &Theme, e: &QuickFilterEditor) -> Node<Msg> {
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Name"))
        .child(field(
            state,
            theme,
            &e.name,
            Field::QuickFilterName,
            "e.g. My bugs",
        ));
    if let Some(jql) = &e.jql {
        body = body
            .child(label(theme, "JQL"))
            .child(area(
                state,
                theme,
                jql,
                Field::QuickFilterJql,
                ("e.g. assignee = currentUser() AND type = Bug", JQL_LINES),
            ))
            .child(hint(
                theme,
                if matches!(e.editing, Some(QuickRef::Kept(_))) {
                    "Jira evaluates it. Urushi keeps this one on the board, as Jira didn't take it."
                } else {
                    "Saved to the board in Jira, which evaluates it."
                },
            ));
    } else {
        body = body.child(label(theme, "Filters by")).child(Node::text(
            e.shown_rule().map(describe).unwrap_or_default(),
            Font::Ui,
            theme.font_size,
            theme.text,
        ));
        let can_replace = e.editing.is_some()
            && state.workspace.board(&e.board).is_some_and(|b| {
                crate::filter::quick::rule_from_selection(&state.board.filter, b).is_some()
            });
        if can_replace {
            body = body.child(button(
                theme,
                "Use current filters",
                ButtonKind::Ghost,
                Msg::Filter(FilterMsg::UseCurrent),
            ));
        }
    }
    if let Some(error) = &e.error {
        body = body.child(Node::text(
            error.as_str(),
            Font::Ui,
            theme.font_size_small,
            theme.danger,
        ));
    }
    let title = if e.editing.is_some() {
        "Edit quick filter"
    } else {
        "Save quick filter"
    };
    let save = if e.busy { "Saving…" } else { "Save" };
    modal(
        theme,
        title,
        460.0,
        body,
        vec![
            cancel(theme),
            button(theme, save, ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}

/// Asking before deleting a quick filter; a Jira board's own goes from Jira too.
pub(in crate::view) fn delete(theme: &Theme, name: &str, filter: &QuickRef) -> Node<Msg> {
    let what = match filter {
        QuickRef::Jira(_) => "It is deleted from the board in Jira too.",
        QuickRef::Kept(_) => "It is deleted from this board.",
    };
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(Node::text(what, Font::Ui, theme.font_size, theme.text));
    modal(
        theme,
        &format!("Delete quick filter “{name}”?"),
        420.0,
        body,
        vec![
            cancel(theme),
            button(
                theme,
                "Delete",
                ButtonKind::Danger,
                Msg::Filter(FilterMsg::ConfirmDelete),
            ),
        ],
        Some(Msg::CloseDialog),
    )
}
