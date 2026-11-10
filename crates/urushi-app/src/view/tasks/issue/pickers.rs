//! The lists a Jira field opens: the transitions a status can take, a field's allowed
//! values, people (searched in Jira as you press Enter, narrowed as you type) and
//! issues (for the parent, or a new link).

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::widgets::{SelectOption, select_options};
use urushi_ui::{Key, Length, Node, Theme, key};

use super::super::super::dialogs::field;
use crate::issue::IssueEditor;
use crate::msg::{IssueMsg, Msg};
use crate::state::{AppState, Field};

type Kind = v1::JiraFieldKind;

/// Options shown before a list scrolls.
const VISIBLE: usize = 7;

/// Key of an open picker's scrolling list.
pub(crate) fn picker_scroll_key() -> Key {
    key(&["issue-picker"])
}

fn list(theme: &Theme, options: Vec<SelectOption<Msg>>, scroll: &HashMap<Key, f32>) -> Node<Msg> {
    let offset = scroll.get(&picker_scroll_key()).copied().unwrap_or(0.0);
    select_options(theme, options, (picker_scroll_key(), offset), VISIBLE)
}

/// The statuses the issue can move to now.
pub(super) fn transitions(
    theme: &Theme,
    details: &v1::JiraIssueDetails,
    scroll: &HashMap<Key, f32>,
) -> Node<Msg> {
    let options = details
        .transitions
        .iter()
        .map(|t| SelectOption {
            label: if t.name == t.to_status || t.to_status.chars().all(|c| c.is_ascii_digit()) {
                t.name.clone()
            } else {
                format!("{} → {}", t.name, t.to_status)
            },
            selected: false,
            on_pick: Msg::Issue(IssueMsg::Transition(t.id.clone())),
        })
        .collect();
    list(theme, options, scroll)
}

/// What `query` (the picker's search, ignoring case) keeps of `name`.
fn matches(query: &str, name: &str) -> bool {
    name.to_lowercase().contains(&query.trim().to_lowercase())
}

/// One value for a field of one: an allowed value, a person, an issue, or none.
pub(super) fn single(
    state: &AppState,
    theme: &Theme,
    (f, kind): (&v1::JiraField, Kind),
    editor: &IssueEditor,
    scroll: &HashMap<Key, f32>,
) -> Node<Msg> {
    let current = editor.value(f);
    let chosen = |id: &str| current.options.iter().any(|o| o.id == id);
    let pick = |option: Option<v1::JiraOption>| {
        Msg::Issue(IssueMsg::Pick {
            field: f.id.clone(),
            option,
        })
    };
    let mut options = Vec::new();
    if !f.required {
        let none = if kind == Kind::User {
            "Unassigned"
        } else {
            "None"
        };
        options.push(SelectOption {
            label: none.to_owned(),
            selected: current.options.is_empty() && current.text.is_empty(),
            on_pick: pick(None),
        });
    }
    let query = editor.search.text();
    let mut column = Node::column().width(Length::Fill(1.0)).gap(6.0);
    match kind {
        Kind::User => {
            column = column.child(search_box(state, theme, editor, "Find a person (Enter)"));
            if f.id == "assignee" {
                options.push(SelectOption {
                    label: "Assign to me".into(),
                    selected: false,
                    on_pick: Msg::Issue(IssueMsg::AssignToMe),
                });
            }
            options.extend(
                editor
                    .people
                    .iter()
                    .filter(|p| matches(query, &p.name))
                    .map(|p| SelectOption {
                        label: p.name.clone(),
                        selected: chosen(&p.account_id),
                        on_pick: pick(Some(v1::JiraOption {
                            id: p.account_id.clone(),
                            name: p.name.clone(),
                        })),
                    }),
            );
        }
        Kind::Issue => {
            column = column.child(search_box(state, theme, editor, "Find an issue (Enter)"));
            options.extend(editor.issues.iter().map(|i| SelectOption {
                label: format!("{} {}", i.key, i.summary),
                selected: current.text == i.key,
                on_pick: pick(Some(v1::JiraOption {
                    id: i.key.clone(),
                    name: i.summary.clone(),
                })),
            }));
        }
        _ => options.extend(f.allowed.iter().map(|o| SelectOption {
            label: o.name.clone(),
            selected: chosen(&o.id),
            on_pick: pick(Some(o.clone())),
        })),
    }
    column.child(list(theme, options, scroll))
}

/// Values to add to (or take from) a field of several.
pub(super) fn several(
    state: &AppState,
    theme: &Theme,
    (f, kind): (&v1::JiraField, Kind),
    editor: &IssueEditor,
    scroll: &HashMap<Key, f32>,
) -> Node<Msg> {
    let current = editor.value(f);
    let toggle = |option: v1::JiraOption| {
        Msg::Issue(IssueMsg::Toggle {
            field: f.id.clone(),
            option,
        })
    };
    let query = editor.search.text();
    let mut column = Node::column().width(Length::Fill(1.0)).gap(6.0);
    let options: Vec<SelectOption<Msg>> = match kind {
        Kind::Labels => {
            column = column.child(search_box(state, theme, editor, "A label (Enter adds it)"));
            Vec::new()
        }
        Kind::Users => {
            column = column.child(search_box(state, theme, editor, "Find a person (Enter)"));
            editor
                .people
                .iter()
                .filter(|p| matches(query, &p.name))
                .map(|p| {
                    let option = v1::JiraOption {
                        id: p.account_id.clone(),
                        name: p.name.clone(),
                    };
                    SelectOption {
                        label: p.name.clone(),
                        selected: current.options.iter().any(|o| o.id == p.account_id),
                        on_pick: toggle(option),
                    }
                })
                .collect()
        }
        _ => f
            .allowed
            .iter()
            .map(|o| SelectOption {
                label: o.name.clone(),
                selected: current.options.iter().any(|c| c.id == o.id),
                on_pick: toggle(o.clone()),
            })
            .collect(),
    };
    if options.is_empty() {
        return column;
    }
    column.child(list(theme, options, scroll))
}

/// Issues to link to, as found.
pub(super) fn link_targets(
    state: &AppState,
    theme: &Theme,
    editor: &IssueEditor,
    scroll: &HashMap<Key, f32>,
) -> Node<Msg> {
    let options: Vec<SelectOption<Msg>> = editor
        .issues
        .iter()
        .map(|i| SelectOption {
            label: format!("{} {}", i.key, i.summary),
            selected: false,
            on_pick: Msg::Issue(IssueMsg::Link(i.key.clone())),
        })
        .collect();
    let mut column = Node::column()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .child(search_box(state, theme, editor, "Find an issue (Enter)"));
    if !options.is_empty() {
        column = column.child(list(theme, options, scroll));
    }
    column
}

fn search_box(
    state: &AppState,
    theme: &Theme,
    editor: &IssueEditor,
    placeholder: &str,
) -> Node<Msg> {
    field(state, theme, &editor.search, Field::JiraSearch, placeholder)
}
