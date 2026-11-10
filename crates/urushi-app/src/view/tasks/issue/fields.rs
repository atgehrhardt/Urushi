//! The right side of a Jira issue's editor: its key (Open in Jira, Watch), its status
//! (a transition, made at once), then every field Jira lets the account edit, each as
//! its kind needs: text, a date, a number, a choice, several, a person, an issue.
//! Changes are drafts until Save; a changed field is marked.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, select_button};
use urushi_ui::{Align, Font, Length, Node, Theme, key};

use super::super::super::dialogs::{area, field, label};
use super::super::super::labels::{tag_chip, tag_chip_width, wrap};
use super::pickers;
use crate::issue::{IssueEditor, STATUS};
use crate::msg::{IssueMsg, JiraMsg, Msg};
use crate::state::{AppState, Field};

type Kind = v1::JiraFieldKind;

/// The issue's own side of the editor, once read.
pub(in crate::view::tasks) fn section(
    state: &AppState,
    theme: &Theme,
    (issue, width): (&v1::JiraIssue, f32),
    editor: &IssueEditor,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Vec<Node<Msg>> {
    let Some(details) = editor.details.as_ref() else {
        return Vec::new();
    };
    let watch = if details.watching {
        format!("Watching ({})", details.watchers.len())
    } else {
        "Watch".to_owned()
    };
    let mut out = vec![
        Node::row()
            .width(Length::Fill(1.0))
            .gap(6.0)
            .align(Align::Center)
            .child(
                Node::text(&issue.key, Font::UiSemibold, theme.font_size, theme.text)
                    .width(Length::Fill(1.0)),
            )
            .child(button(
                theme,
                &watch,
                ButtonKind::Ghost,
                Msg::Issue(IssueMsg::Watch(!details.watching)),
            ))
            .child(button(
                theme,
                "Open in Jira",
                ButtonKind::Ghost,
                Msg::Jira(JiraMsg::Open(issue.url.clone())),
            )),
        label(theme, "Status"),
        select_button(
            theme,
            &issue.status,
            editor.open.as_deref() == Some(STATUS),
            key(&["issue-status"]),
            Msg::Issue(IssueMsg::Open(Some(STATUS.into()))),
        ),
    ];
    if editor.open.as_deref() == Some(STATUS) {
        out.push(pickers::transitions(theme, details, scroll));
    }
    out.extend(fields(state, theme, editor, width, scroll));
    out
}

/// A new Jira task's own fields (required ones marked), once read.
pub(in crate::view::tasks) fn create_section(
    state: &AppState,
    theme: &Theme,
    editor: &IssueEditor,
    width: f32,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Vec<Node<Msg>> {
    if editor.loading && editor.details.is_none() {
        return vec![label(theme, "Loading Jira's fields…")];
    }
    fields(state, theme, editor, width, scroll)
}

fn fields(
    state: &AppState,
    theme: &Theme,
    editor: &IssueEditor,
    width: f32,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Vec<Node<Msg>> {
    let mut out = Vec::new();
    for (i, f) in editor.fields().iter().enumerate() {
        let Ok(i) = u16::try_from(i) else {
            break;
        };
        let changed = editor.drafts.contains_key(&f.id)
            || editor
                .texts
                .get(&f.id)
                .is_some_and(|t| f.value.as_ref().is_none_or(|v| v.text != t.text()));
        let mut name = f.name.clone();
        if f.required {
            name.push_str(" *");
        }
        if changed {
            name.push_str("  •");
        }
        out.push(label(theme, &name));
        out.extend(control(state, theme, (i, f), editor, width, scroll));
    }
    out
}

/// The names of a value's options, or `none`.
fn names(value: &v1::JiraFieldValue, none: &str) -> String {
    if value.options.is_empty() {
        none.to_owned()
    } else {
        value
            .options
            .iter()
            .map(|o| o.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn control(
    state: &AppState,
    theme: &Theme,
    (i, f): (u16, &v1::JiraField),
    editor: &IssueEditor,
    width: f32,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Vec<Node<Msg>> {
    let value = editor.value(f);
    let open = editor.open.as_deref() == Some(f.id.as_str());
    let toggle = Msg::Issue(IssueMsg::Open(Some(f.id.clone())));
    let typed = || {
        editor
            .text_at(i)
            .cloned()
            .unwrap_or_else(|| urushi_ui::TextField::new(value.text.clone()))
    };
    let kind = Kind::try_from(f.kind).unwrap_or(Kind::Other);
    match kind {
        Kind::Text | Kind::Number | Kind::Date | Kind::DateTime => {
            let hint = match kind {
                Kind::Number => "A number",
                Kind::Date => "YYYY-MM-DD",
                Kind::DateTime => "YYYY-MM-DDThh:mm",
                _ => "",
            };
            vec![field(state, theme, &typed(), Field::JiraText(i), hint)]
        }
        Kind::RichText => vec![area(
            state,
            theme,
            &typed(),
            Field::JiraText(i),
            ("Markdown", 3),
        )],
        Kind::Option | Kind::Sprint | Kind::User | Kind::Issue => {
            let none = match kind {
                Kind::User => "Unassigned",
                _ => "None",
            };
            let shown = if kind == Kind::Issue && !value.text.is_empty() {
                value.options.first().map_or_else(
                    || value.text.clone(),
                    |o| format!("{} {}", value.text, o.name),
                )
            } else {
                names(&value, none)
            };
            let mut out = vec![select_button(
                theme,
                &shown,
                open,
                key(&["issue-field", &f.id]),
                toggle,
            )];
            if open {
                out.push(pickers::single(state, theme, (f, kind), editor, scroll));
            }
            out
        }
        Kind::Options | Kind::Users | Kind::Labels => {
            let chips: Vec<(f32, Node<Msg>)> = value
                .options
                .iter()
                .map(|o| {
                    let remove = Msg::Issue(IssueMsg::Toggle {
                        field: f.id.clone(),
                        option: o.clone(),
                    });
                    let size = theme.font_size_small;
                    (
                        tag_chip_width(&o.name, size, true),
                        tag_chip(theme, &o.name, size, Some(remove)),
                    )
                })
                .collect();
            let mut out = Vec::new();
            if !chips.is_empty() {
                out.push(wrap(chips, 6.0, width));
            }
            out.push(select_button(
                theme,
                "Add…",
                open,
                key(&["issue-field", &f.id]),
                toggle,
            ));
            if open {
                out.push(pickers::several(state, theme, (f, kind), editor, scroll));
            }
            out
        }
        Kind::Unspecified | Kind::Other => vec![Node::text(
            if value.text.is_empty() {
                "—".to_owned()
            } else {
                value.text
            },
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        )],
    }
}
