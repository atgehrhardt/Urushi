//! Under a Jira issue's notes: its comments (add one, change or delete your own), its
//! files (open, add, delete), its links to other issues (open one, add, remove) and its
//! work log (time tracking, and logging time). Each change is made in Jira at once.

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, SelectOption, button, select_button, select_options};
use urushi_ui::{Align, Color, Font, Insets, Key, Length, Node, Role, Theme, key};

use super::super::super::dialogs::{area, field, hint};
use super::{byline, day, paragraph, pickers, size, small};
use crate::issue::{IssueEditor, IssueTab, LINK, words};
use crate::msg::{BoardMsg, IssueMsg, JiraMsg, Msg};
use crate::state::{AppState, Field};

/// Comments shown in full; older ones are summed up.
const COMMENTS: usize = 20;

/// The tab row and the tab shown, for the issue of task `task`.
pub(in crate::view::tasks) fn tabs(
    state: &AppState,
    theme: &Theme,
    (task, width): (&str, f32),
    editor: &IssueEditor,
    scroll: &HashMap<Key, f32>,
) -> Vec<Node<Msg>> {
    let Some(d) = editor.details.as_ref() else {
        let text = if editor.loading {
            "Loading the issue from Jira…"
        } else {
            "Comments, files and links show once Jira answers."
        };
        return vec![hint(theme, text)];
    };
    let tab = |t: IssueTab, name: String| {
        let on = editor.tab == t;
        Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .radius(theme.radius)
            .background(if on {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(theme.hover)
            .key(key(&["issue-tab", &name]))
            .on_click(Msg::Issue(IssueMsg::Tab(t)))
            .role(Role::Item, name.clone())
            .child(Node::text(
                name,
                Font::UiMedium,
                theme.font_size_small,
                if on { theme.text } else { theme.text_muted },
            ))
    };
    let count = |n: usize, name: &str| {
        if n == 0 {
            name.to_owned()
        } else {
            format!("{name} {n}")
        }
    };
    let row = Node::row()
        .gap(4.0)
        .child(tab(IssueTab::Comments, count(d.comments.len(), "Comments")))
        .child(tab(IssueTab::Files, count(d.attachments.len(), "Files")))
        .child(tab(IssueTab::Links, count(d.links.len(), "Links")))
        .child(tab(IssueTab::Work, count(d.worklogs.len(), "Work")));
    let mut out = vec![row];
    out.extend(match editor.tab {
        IssueTab::Comments => comments(state, theme, d, editor, width),
        IssueTab::Files => files(theme, task, d),
        IssueTab::Links => links(state, theme, d, editor, scroll),
        IssueTab::Work => work(state, theme, d, editor),
    });
    out
}

fn comments(
    state: &AppState,
    theme: &Theme,
    d: &v1::JiraIssueDetails,
    editor: &IssueEditor,
    width: f32,
) -> Vec<Node<Msg>> {
    let mut out = Vec::new();
    let skipped = d.comments.len().saturating_sub(COMMENTS);
    if skipped > 0 {
        out.push(hint(
            theme,
            &format!("{skipped} older comments are in Jira."),
        ));
    }
    for c in d.comments.iter().skip(skipped) {
        let who = c.author.as_ref().map_or("Someone", |a| a.name.as_str());
        let mut head = Node::row()
            .width(Length::Fill(1.0))
            .align(Align::Center)
            .child(byline(theme, who, &day(c.created_ms)).width(Length::Fill(1.0)));
        if c.mine {
            head = head
                .child(small(
                    theme,
                    "Edit",
                    theme.text_muted,
                    &format!("Edit comment {}", c.id),
                    Msg::Issue(IssueMsg::EditComment(c.id.clone())),
                ))
                .child(small(
                    theme,
                    "Delete",
                    theme.danger,
                    &format!("Delete comment {}", c.id),
                    Msg::Issue(IssueMsg::DeleteComment(c.id.clone())),
                ));
        }
        out.push(head);
        match &editor.editing {
            Some((id, text)) if *id == c.id => {
                out.push(area(
                    state,
                    theme,
                    text,
                    Field::JiraCommentEdit,
                    ("Markdown", 3),
                ));
                out.push(
                    Node::row()
                        .gap(6.0)
                        .child(button(
                            theme,
                            "Save comment",
                            ButtonKind::Secondary,
                            Msg::Issue(IssueMsg::SaveComment),
                        ))
                        .child(button(
                            theme,
                            "Cancel",
                            ButtonKind::Ghost,
                            Msg::Issue(IssueMsg::CancelComment),
                        )),
                );
            }
            _ => out.push(paragraph(theme, &c.body, width, 10)),
        }
    }
    out.push(area(
        state,
        theme,
        &editor.comment,
        Field::JiraComment,
        ("Add a comment… (Markdown)", 2),
    ));
    out.push(Node::row().child(button(
        theme,
        "Comment",
        ButtonKind::Secondary,
        Msg::Issue(IssueMsg::Comment),
    )));
    out
}

fn files(theme: &Theme, task: &str, d: &v1::JiraIssueDetails) -> Vec<Node<Msg>> {
    let mut out: Vec<Node<Msg>> = d
        .attachments
        .iter()
        .map(|a| {
            Node::row()
                .width(Length::Fill(1.0))
                .gap(8.0)
                .align(Align::Center)
                .child(
                    Node::text(&a.name, Font::Ui, theme.font_size, theme.text)
                        .width(Length::Fill(1.0)),
                )
                .child(Node::text(
                    size(a.size),
                    Font::Ui,
                    theme.font_size_small,
                    theme.text_muted,
                ))
                .child(small(
                    theme,
                    "Open",
                    theme.accent,
                    &format!("Open {}", a.name),
                    Msg::Issue(IssueMsg::OpenAttachment(a.id.clone(), a.name.clone())),
                ))
                .child(small(
                    theme,
                    "×",
                    theme.text_muted,
                    &format!("Delete {}", a.name),
                    Msg::Issue(IssueMsg::DeleteAttachment(a.id.clone())),
                ))
        })
        .collect();
    if out.is_empty() {
        out.push(hint(theme, "No files."));
    }
    if AppState::can_pick_files() {
        out.push(Node::row().child(button(
            theme,
            "Add files…",
            ButtonKind::Secondary,
            Msg::Board(BoardMsg::AddFiles(task.to_owned())),
        )));
    }
    out
}

fn links(
    state: &AppState,
    theme: &Theme,
    d: &v1::JiraIssueDetails,
    editor: &IssueEditor,
    scroll: &HashMap<Key, f32>,
) -> Vec<Node<Msg>> {
    let mut out = Vec::new();
    for l in &d.links {
        let Some(other) = l.issue.as_ref() else {
            continue;
        };
        // An issue on a board here opens in the editor; others in Jira.
        let open = state
            .workspace
            .all_tasks()
            .find(|t| t.issue().is_some_and(|i| i.key == other.key))
            .map_or_else(
                || Msg::Jira(JiraMsg::Open(other.url.clone())),
                |t| Msg::Board(BoardMsg::OpenTask(t.id().to_owned())),
            );
        out.push(
            Node::row()
                .width(Length::Fill(1.0))
                .gap(8.0)
                .align(Align::Center)
                .child(Node::text(
                    &l.relation,
                    Font::Ui,
                    theme.font_size_small,
                    theme.text_muted,
                ))
                .child(
                    Node::row()
                        .width(Length::Fill(1.0))
                        .gap(6.0)
                        .radius(4.0)
                        .hover_background(theme.hover)
                        .key(key(&["issue-link", &l.id]))
                        .on_click(open)
                        .role(Role::Button, format!("Open {}", other.key))
                        .child(Node::text(
                            &other.key,
                            Font::UiMedium,
                            theme.font_size_small,
                            theme.accent,
                        ))
                        .child(
                            Node::text(&other.summary, Font::Ui, theme.font_size_small, theme.text)
                                .width(Length::Fill(1.0)),
                        ),
                )
                .child(Node::text(
                    &other.status,
                    Font::Ui,
                    theme.font_size_small,
                    theme.text_muted,
                ))
                .child(small(
                    theme,
                    "×",
                    theme.text_muted,
                    &format!("Unlink {}", other.key),
                    Msg::Issue(IssueMsg::Unlink(l.id.clone())),
                )),
        );
    }
    if d.link_types.is_empty() {
        return out;
    }
    let open = editor.open.as_deref() == Some(LINK);
    out.push(select_button(
        theme,
        "Link an issue…",
        open,
        key(&["issue-link-add"]),
        Msg::Issue(IssueMsg::Open(Some(LINK.into()))),
    ));
    if open {
        // "This issue blocks / is blocked by …": each kind from both sides.
        let sides: Vec<SelectOption<Msg>> = d
            .link_types
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                [(true, &t.outward), (false, &t.inward)]
                    .into_iter()
                    .map(move |(outward, words)| (i, outward, words.clone()))
            })
            .map(|(i, outward, words)| SelectOption {
                label: format!("This issue {words}"),
                selected: editor.link == (i, outward),
                on_pick: Msg::Issue(IssueMsg::LinkKind(i, outward)),
            })
            .collect();
        let offset = scroll
            .get(&key(&["issue-link-kinds"]))
            .copied()
            .unwrap_or(0.0);
        out.push(select_options(
            theme,
            sides,
            (key(&["issue-link-kinds"]), offset),
            4,
        ));
        out.push(pickers::link_targets(state, theme, editor, scroll));
    }
    out
}

fn work(
    state: &AppState,
    theme: &Theme,
    d: &v1::JiraIssueDetails,
    editor: &IssueEditor,
) -> Vec<Node<Msg>> {
    let mut out = Vec::new();
    let tracked: Vec<String> = [
        ("Estimate", &d.original_estimate),
        ("Remaining", &d.remaining_estimate),
        ("Spent", &d.time_spent),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(name, v)| format!("{name} {v}"))
    .collect();
    if !tracked.is_empty() {
        out.push(hint(theme, &tracked.join(" · ")));
    }
    for w in &d.worklogs {
        let who = w.author.as_ref().map_or("Someone", |a| a.name.as_str());
        out.push(
            Node::row()
                .width(Length::Fill(1.0))
                .gap(8.0)
                .child(byline(theme, who, &day(w.started_ms)).width(Length::Fill(1.0)))
                .child(Node::text(
                    words(w.seconds),
                    Font::UiMedium,
                    theme.font_size_small,
                    theme.text,
                )),
        );
    }
    out.push(
        Node::row()
            .width(Length::Fill(1.0))
            .gap(6.0)
            .align(Align::Center)
            .child(
                field(
                    state,
                    theme,
                    &editor.work,
                    Field::JiraWork,
                    "Time spent, e.g. 1h 30m",
                )
                .width(Length::Fill(1.0)),
            )
            .child(button(
                theme,
                "Log work",
                ButtonKind::Secondary,
                Msg::Issue(IssueMsg::LogWork),
            )),
    );
    out
}
