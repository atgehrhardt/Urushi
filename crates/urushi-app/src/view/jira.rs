//! Jira's dialogs (protocol 1.12): signing the daemon in to a Jira Cloud site, choosing
//! a board to import, and the Jira rows of Settings.

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, modal};
use urushi_ui::{Font, Insets, Key, Length, Node, Role, TextField, Theme, key};

use super::board::shorten;
use super::dialogs::{cancel, field, hint, label};
use crate::msg::{JiraMsg, Msg};
use crate::state::{AppState, Field};

/// Rows of boards shown before the list scrolls.
const VISIBLE: usize = 8;
/// Height of a board row.
const ROW: f32 = 34.0;
/// Where API tokens are made.
const TOKENS_URL: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

/// Key of the import list's scroll.
pub(crate) fn import_scroll_key() -> Key {
    key(&["jira-boards"])
}

fn error_line(theme: &Theme, error: Option<&str>) -> Option<Node<Msg>> {
    error.map(|e| Node::text(e, Font::Ui, theme.font_size_small, theme.danger))
}

/// Signing in: the site, the account's email and an API token (hidden as it's typed).
pub(super) fn connect(
    state: &AppState,
    theme: &Theme,
    (site, email, token): (&TextField, &TextField, &TextField),
    (busy, error): (bool, Option<&str>),
) -> Node<Msg> {
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Site"))
        .child(field(
            state,
            theme,
            site,
            Field::JiraSite,
            "acme.atlassian.net",
        ))
        .child(label(theme, "Email"))
        .child(field(
            state,
            theme,
            email,
            Field::JiraEmail,
            "you@example.com",
        ))
        .child(label(theme, "API token"))
        .child(field(
            state,
            theme,
            token,
            Field::JiraToken,
            "Paste an API token",
        ))
        .child(
            Node::row()
                .gap(6.0)
                .child(hint(
                    theme,
                    "Make one at id.atlassian.com → Security → API tokens.",
                ))
                .child(
                    Node::row()
                        .padding(Insets::symmetric(0.0, 2.0))
                        .key(key(&["jira-tokens-link"]))
                        .on_click(Msg::Jira(JiraMsg::Open(TOKENS_URL.into())))
                        .role(Role::Button, "Open the API tokens page")
                        .child(Node::text(
                            "Open",
                            Font::UiMedium,
                            theme.font_size_small,
                            theme.accent,
                        )),
                ),
        )
        .child(hint(
            theme,
            "The daemon keeps the token, readable only by you, and uses it to sync boards.",
        ))
        .children(error_line(theme, error));
    let action = if busy { "Checking…" } else { "Connect" };
    modal(
        theme,
        "Connect Jira",
        480.0,
        body,
        vec![
            cancel(theme),
            button(theme, action, ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}

/// Choosing a board to import: a search (sent with Enter) and the boards found.
pub(super) fn import(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    query: &TextField,
    (boards, error): (Option<&[v1::JiraBoardInfo]>, Option<&str>),
) -> Node<Msg> {
    let site = state
        .jira
        .as_ref()
        .map(|a| a.site.trim_start_matches("https://").to_owned())
        .unwrap_or_default();
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(field(
            state,
            theme,
            query,
            Field::JiraQuery,
            "Search boards (Enter)…",
        ));
    match boards {
        None => body = body.child(hint(theme, &format!("Loading boards from {site}…"))),
        Some([]) if error.is_none() => {
            body = body.child(hint(theme, "No board matches."));
        }
        Some(boards) => {
            let rows: Vec<Node<Msg>> = boards.iter().map(|b| board_row(theme, b)).collect();
            let mut list = Node::column().width(Length::Fill(1.0));
            if rows.len() > VISIBLE {
                #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
                let height = VISIBLE as f32 * ROW;
                let offset = scroll.get(&import_scroll_key()).copied().unwrap_or(0.0);
                list = list
                    .height(Length::Fixed(height))
                    .scroll(import_scroll_key(), offset);
            }
            body = body.child(list.children(rows));
        }
    }
    body = body.children(error_line(theme, error)).child(hint(
        theme,
        "The board keeps Jira's columns and stays in sync: changes made here go to Jira.",
    ));
    modal(
        theme,
        "Import a Jira board",
        520.0,
        body,
        vec![cancel(theme)],
        Some(Msg::CloseDialog),
    )
}

/// One board: its name, its kind and project; imported ones are shown, not offered.
fn board_row(theme: &Theme, b: &v1::JiraBoardInfo) -> Node<Msg> {
    let imported = !b.imported_board_id.is_empty();
    let detail = format!(
        "{} · {}{}",
        if b.scrum { "Scrum" } else { "Kanban" },
        if b.project_name.is_empty() {
            &b.project_key
        } else {
            &b.project_name
        },
        if imported { " · imported" } else { "" }
    );
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(ROW))
        .padding(Insets::symmetric(0.0, 8.0))
        .gap(10.0)
        .radius(theme.radius)
        .align(urushi_ui::Align::Center)
        .key(key(&["jira-board", &b.id.to_string()]))
        .role(Role::Button, format!("Import {}", b.name))
        .child(
            Node::text(
                shorten(&b.name, 40),
                Font::UiMedium,
                theme.font_size,
                if imported {
                    theme.text_muted
                } else {
                    theme.text
                },
            )
            .width(Length::Fill(1.0)),
        )
        .child(Node::text(
            detail,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ));
    if !imported {
        row = row
            .hover_background(theme.hover)
            .on_click(Msg::Jira(JiraMsg::Import(b.id)));
    }
    row
}

/// Settings' Jira rows: who the daemon is signed in as, or a way to sign in.
pub(super) fn settings_rows(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if !state.daemon_supports_jira() {
        return Vec::new();
    }
    let account = state.jira.clone().unwrap_or_default();
    let (status, action) = if account.connected {
        (
            format!(
                "Signed in to {} as {}",
                account.site.trim_start_matches("https://"),
                if account.display_name.is_empty() {
                    &account.email
                } else {
                    &account.display_name
                }
            ),
            button(
                theme,
                "Disconnect",
                ButtonKind::Ghost,
                Msg::Jira(JiraMsg::Disconnect),
            ),
        )
    } else {
        (
            "Not connected. Import Jira boards and keep them in sync.".to_owned(),
            button(
                theme,
                "Connect Jira…",
                ButtonKind::Secondary,
                Msg::Jira(JiraMsg::OpenConnect),
            ),
        )
    };
    vec![
        label(theme, "Jira"),
        Node::row()
            .width(Length::Fill(1.0))
            .gap(8.0)
            .align(urushi_ui::Align::Center)
            .child(
                Node::text(status, Font::Ui, theme.font_size_small, theme.text)
                    .width(Length::Fill(1.0)),
            )
            .child(action),
    ]
}
