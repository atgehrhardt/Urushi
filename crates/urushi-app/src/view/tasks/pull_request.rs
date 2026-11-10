//! The task editor's pull request link (ADR 0036): on a local board, the URL of the
//! pull request for the task, shown on its thread's card. A Jira issue's pull requests
//! come from Jira, so a Jira board has no such field.

use urushi_core::board::PrUrl;
use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Align, Font, Length, Node, Theme};

use super::super::dialogs::{field, label};
use crate::board::TaskLabels;
use crate::msg::Msg;
use crate::state::{AppState, Field};

/// "Pull request", its field, and an Open button once it holds an address a browser
/// can open (or a note saying what it needs).
pub(super) fn section(state: &AppState, theme: &Theme, labels: &TaskLabels) -> Vec<Node<Msg>> {
    let typed = labels.pr_url.text().trim();
    let input = field(
        state,
        theme,
        &labels.pr_url,
        Field::TaskPrUrl,
        "https://github.com/…/pull/12",
    )
    .width(Length::Fill(1.0));
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center)
        .child(input);
    let mut out = vec![label(theme, "Pull request")];
    match PrUrl::parse(typed) {
        Ok(url) => {
            row = row.child(button(
                theme,
                "Open",
                ButtonKind::Ghost,
                Msg::OpenUrl(url.as_str().to_owned()),
            ));
            out.push(row);
        }
        Err(_) if typed.is_empty() => out.push(row),
        Err(_) => {
            out.push(row);
            out.push(Node::text(
                "Paste the pull request's https:// address",
                Font::Ui,
                theme.font_size_small,
                theme.danger,
            ));
        }
    }
    out
}
