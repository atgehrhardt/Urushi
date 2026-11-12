//! Jira's dialogs (protocol 1.12): signing the daemon in to a Jira Cloud site, choosing
//! a board to import (from which sign-in's site, when there are several: ADR 0049), and
//! the Jira rows of Settings, one per sign-in.

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button};
use urushi_ui::{Font, Insets, Key, Length, Node, Role, TextField, Theme, key};

use super::board::shorten;
use super::dialogs::{cancel, field, hint, label};
use crate::msg::{JiraMsg, Msg};
use crate::state::{AppState, Field};
use crate::view::modal::modal;

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

/// The Jira dialogs: signing in and importing a board (`None` for any other).
pub(super) fn dialog(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    dialog: &crate::state::Dialog,
) -> Option<Node<Msg>> {
    use crate::state::Dialog;
    Some(match dialog {
        Dialog::ConnectJira {
            site,
            email,
            token,
            busy,
            error,
        } => connect(
            state,
            theme,
            (site, email, token),
            (*busy, error.as_deref()),
        ),
        Dialog::ImportJira {
            account,
            query,
            boards,
            error,
        } => import(
            state,
            (theme, scroll),
            (account, query),
            (boards.as_deref(), error.as_deref()),
        ),
        _ => return None,
    })
}

/// Signing in: the site, the account's email and an API token (hidden as it's typed).
fn connect(
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
        state,
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

/// Choosing a board to import: with several sign-ins, which one's site (and adding
/// another); a search (sent with Enter) and the boards found.
fn import(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    (account, query): (&str, &TextField),
    (boards, error): (Option<&[v1::JiraBoardInfo]>, Option<&str>),
) -> Node<Msg> {
    let chosen = state
        .jira_accounts
        .iter()
        .find(|a| a.key == account)
        .or_else(|| state.jira_accounts.first());
    let site = chosen.map(|a| host(&a.site)).unwrap_or_default();
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .children(account_picker(state, theme, chosen))
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
        state,
        theme,
        "Import a Jira board",
        520.0,
        body,
        vec![cancel(theme)],
        Some(Msg::CloseDialog),
    )
}

/// A site's host, as people write it: `acme.atlassian.net`.
fn host(site: &str) -> &str {
    site.trim_start_matches("https://")
}

/// With more than one sign-in (1.27 daemons), a chip per sign-in's site choosing whose
/// boards are listed, and "+ Account" to sign in to another.
fn account_picker(
    state: &AppState,
    theme: &Theme,
    chosen: Option<&v1::JiraAccount>,
) -> Option<Node<Msg>> {
    if !state.daemon_supports_jira_accounts() {
        return None;
    }
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .align(urushi_ui::Align::Center);
    if state.jira_accounts.len() > 1 {
        for a in &state.jira_accounts {
            let on = chosen.is_some_and(|c| c.key == a.key);
            row = row.child(
                Node::row()
                    .padding(Insets::symmetric(4.0, 10.0))
                    .radius(theme.radius)
                    .border(1.0, if on { theme.accent } else { theme.border })
                    .background(if on {
                        theme.accent.with_alpha(0.18)
                    } else {
                        urushi_ui::Color::TRANSPARENT
                    })
                    .hover_background(if on {
                        theme.accent.with_alpha(0.28)
                    } else {
                        theme.hover
                    })
                    .key(key(&["jira-import-account", &a.key]))
                    .on_click(Msg::Jira(JiraMsg::ImportFrom(a.key.clone())))
                    .role(
                        Role::Switch,
                        format!("Boards of {} as {}", host(&a.site), a.email),
                    )
                    .child(Node::text(
                        shorten(host(&a.site), 28),
                        Font::UiMedium,
                        theme.font_size_small,
                        if on { theme.text } else { theme.text_muted },
                    )),
            );
        }
    } else {
        row = row.child(hint(
            theme,
            &chosen.map_or_else(String::new, |a| {
                format!("From {} as {}", host(&a.site), a.email)
            }),
        ));
    }
    Some(
        row.child(Node::spacer()).child(
            button(
                theme,
                "+ Account",
                ButtonKind::Ghost,
                Msg::Jira(JiraMsg::OpenConnect),
            )
            .key(key(&["jira-add-account"])),
        ),
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

/// Settings' Jira rows: each sign-in (site, name, email) with **Disconnect**, then a way
/// to add one (or, signed in nowhere, to connect).
pub(super) fn settings_rows(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if !state.daemon_supports_jira() {
        return Vec::new();
    }
    let mut rows = vec![label(theme, "Jira")];
    for a in &state.jira_accounts {
        let name = if a.display_name.is_empty() {
            a.email.clone()
        } else {
            format!("{} ({})", a.display_name, a.email)
        };
        rows.push(row(
            theme,
            format!("Signed in to {} as {name}", host(&a.site)),
            button(
                theme,
                "Disconnect",
                ButtonKind::Ghost,
                Msg::Jira(JiraMsg::Disconnect(a.key.clone())),
            )
            .key(key(&["jira-disconnect", &a.key])),
        ));
    }
    if state.jira_accounts.is_empty() {
        rows.push(row(
            theme,
            "Not connected. Import Jira boards and keep them in sync.".to_owned(),
            button(
                theme,
                "Connect Jira…",
                ButtonKind::Secondary,
                Msg::Jira(JiraMsg::OpenConnect),
            ),
        ));
    } else if state.daemon_supports_jira_accounts() {
        rows.push(row(
            theme,
            "Import boards from another Jira site or account.".to_owned(),
            button(
                theme,
                "Add Jira account…",
                ButtonKind::Secondary,
                Msg::Jira(JiraMsg::OpenConnect),
            ),
        ));
    }
    rows
}

/// A Settings row: what it says, and its button.
fn row(theme: &Theme, status: String, action: Node<Msg>) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .align(urushi_ui::Align::Center)
        .child(
            Node::text(status, Font::Ui, theme.font_size_small, theme.text)
                .width(Length::Fill(1.0)),
        )
        .child(action)
}
