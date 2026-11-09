//! The sidebar's Projects tab: each project (collapsible), then threads in no project.
//! Inside each group: pinned threads first, then one collapsible section per status in
//! use, then threads without a status. Threads can be dragged onto a project (or onto
//! "No project") to move them.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use crate::msg::Msg;
use crate::organize::{DropTarget, MenuTarget, section_key};
use crate::state::{AppState, Connection};

/// Every row of the Projects tab.
pub(super) fn list(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if state.workspace.repos().is_empty() {
        return no_repos(state, theme);
    }
    let mut out: Vec<Node<Msg>> = state
        .workspace
        .projects()
        .iter()
        .map(|p| project_block(state, theme, p))
        .collect();
    let ungrouped: Vec<&v1::Thread> = state.workspace.threads_in(None).collect();
    if !ungrouped.is_empty() || state.dragging.is_some() {
        let title = if state.workspace.projects().is_empty() {
            "Threads"
        } else {
            "No project"
        };
        out.push(block(
            state,
            theme,
            Block {
                project: None,
                title,
                threads: ungrouped,
                header_extra: None,
                menu: None,
            },
        ));
    }
    if state.workspace.projects().is_empty() && state.workspace.raw().threads.is_empty() {
        out.push(hint(
            theme,
            "Create a project with + to organize threads, or start a thread from the Repos tab.",
        ));
    }
    out
}

/// What the tab shows before any repo is added.
pub(super) fn no_repos(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if !matches!(state.connection, Connection::Connected { .. }) {
        return Vec::new();
    }
    vec![
        Node::column()
            .padding(Insets::all(8.0))
            .gap(8.0)
            .child(Node::text(
                "No repos yet.",
                Font::Ui,
                theme.font_size,
                theme.text_muted,
            ))
            .child(button(
                theme,
                "Add a repository",
                ButtonKind::Secondary,
                Msg::OpenAddProject,
            )),
    ]
}

fn hint(theme: &Theme, text: &str) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets::symmetric(4.0, 12.0))
        .child(
            Node::text(text, Font::Ui, theme.font_size_small, theme.text_muted)
                .width(Length::Fill(1.0)),
        )
}

fn project_block(state: &AppState, theme: &Theme, project: &v1::Folder) -> Node<Msg> {
    block(
        state,
        theme,
        Block {
            project: Some(&project.id),
            title: &project.name,
            threads: state.workspace.threads_in(Some(&project.id)).collect(),
            header_extra: Some(icon_button(
                theme,
                "+",
                &format!("New thread in {}", project.name),
                Msg::OpenNewThread {
                    repo: None,
                    project: Some(project.id.clone()),
                },
            )),
            menu: Some(MenuTarget::Project(project.id.clone())),
        },
    )
}

struct Block<'a> {
    project: Option<&'a str>,
    title: &'a str,
    threads: Vec<&'a v1::Thread>,
    header_extra: Option<Node<Msg>>,
    menu: Option<MenuTarget>,
}

/// A project (or the ungrouped threads): a collapsible header and its threads. While a
/// thread is dragged, the whole block is a drop target.
fn block(state: &AppState, theme: &Theme, b: Block<'_>) -> Node<Msg> {
    let key_ = section_key(b.project, None);
    let collapsed = state.collapsed.contains(&key_);
    let mut header = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(30.0))
        .padding(Insets {
            top: 6.0,
            right: 4.0,
            bottom: 0.0,
            left: 4.0,
        })
        .gap(6.0)
        .key(key(&["section", &key_]))
        .on_click(Msg::ToggleSection(key_.clone()))
        .role(
            Role::Button,
            format!("{} ({} threads)", b.title, b.threads.len()),
        )
        .child(chevron(theme, collapsed))
        .child(
            Node::text(b.title, Font::UiSemibold, theme.font_size, theme.text)
                .width(Length::Fill(1.0)),
        )
        .child(count(theme, b.threads.len()));
    if let Some(menu) = b.menu {
        header = header.on_secondary_click(Msg::OpenMenu(menu));
    }
    if let Some(extra) = b.header_extra {
        header = header.child(extra);
    }
    let mut column = Node::column()
        .width(Length::Fill(1.0))
        .gap(2.0)
        .radius(theme.radius)
        .padding(Insets {
            top: 0.0,
            right: 0.0,
            bottom: 4.0,
            left: 0.0,
        })
        .child(header);
    if state.dragging.is_some() {
        let target = b
            .project
            .map_or(DropTarget::Ungrouped, |p| DropTarget::Project(p.to_owned()));
        column = column
            .background(Color::TRANSPARENT)
            .hover_background(theme.accent.with_alpha(0.15))
            .border(1.0, theme.border)
            .on_drop(Msg::DropThread(target));
    }
    if collapsed {
        return column;
    }
    if b.threads.is_empty() {
        return column.child(hint(theme, "Drag threads here, or use + for a new one."));
    }
    column.children(group(state, theme, b.project, &b.threads))
}

/// Pinned threads, then a section per status, then threads without a status.
fn group(
    state: &AppState,
    theme: &Theme,
    project: Option<&str>,
    threads: &[&v1::Thread],
) -> Vec<Node<Msg>> {
    let mut out: Vec<Node<Msg>> = threads
        .iter()
        .filter(|t| t.pinned)
        .map(|t| thread_card(state, theme, t))
        .collect();
    let rest: Vec<&v1::Thread> = threads.iter().copied().filter(|t| !t.pinned).collect();
    let mut statuses: Vec<&str> = rest
        .iter()
        .map(|t| t.status.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    statuses.sort_by_key(|s| s.to_lowercase());
    statuses.dedup();
    for status in statuses {
        let members: Vec<&v1::Thread> = rest
            .iter()
            .copied()
            .filter(|t| t.status == status)
            .collect();
        let key_ = section_key(project, Some(status));
        let collapsed = state.collapsed.contains(&key_);
        out.push(
            Node::row()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(24.0))
                .padding(Insets::symmetric(0.0, 12.0))
                .gap(6.0)
                .key(key(&["section", &key_]))
                .on_click(Msg::ToggleSection(key_.clone()))
                .role(
                    Role::Button,
                    format!("Status {status} ({} threads)", members.len()),
                )
                .child(chevron(theme, collapsed))
                .child(
                    Node::text(
                        status.to_uppercase(),
                        Font::UiMedium,
                        theme.font_size_small - 1.0,
                        theme.text_muted,
                    )
                    .width(Length::Fill(1.0)),
                )
                .child(count(theme, members.len())),
        );
        if !collapsed {
            out.extend(members.iter().map(|t| thread_card(state, theme, t)));
        }
    }
    out.extend(
        rest.iter()
            .filter(|t| t.status.is_empty())
            .map(|t| thread_card(state, theme, t)),
    );
    out
}

fn chevron(theme: &Theme, collapsed: bool) -> Node<Msg> {
    Node::text(
        if collapsed { "▸" } else { "▾" },
        Font::Ui,
        theme.font_size + 2.0,
        theme.text_muted,
    )
    .width(Length::Fixed(12.0))
}

fn count(theme: &Theme, n: usize) -> Node<Msg> {
    Node::text(
        n.to_string(),
        Font::Ui,
        theme.font_size_small,
        theme.text_muted,
    )
}

/// A thread: title, then which repo and branch it's on. Click to open, right-click
/// (two-finger click) for its menu, drag to move it to another project.
fn thread_card(state: &AppState, theme: &Theme, thread: &v1::Thread) -> Node<Msg> {
    let selected = state.selected_thread.as_deref() == Some(&thread.id);
    let dragged = state
        .dragging
        .as_ref()
        .is_some_and(|(t, _)| *t == thread.id);
    let badge = match v1::ThreadState::try_from(thread.state) {
        Ok(v1::ThreadState::Ready) => None,
        Ok(v1::ThreadState::DeletionFailed) => Some(("deletion failed", theme.danger)),
        Ok(v1::ThreadState::Deleting) => Some(("deleting", theme.warning)),
        _ => Some(("creating", theme.warning)),
    };
    let attention = state.thread_attention(&thread.id);
    let mut details = Node::column()
        .width(Length::Fill(1.0))
        .gap(1.0)
        .child(thread_title(theme, thread, selected, attention))
        .child(Node::text(
            thread_meta(state, thread),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ));
    // The thread's lifecycle (creating, deleting) matters more than its agent.
    let badge = badge.or_else(|| {
        attention.map(|k| {
            (
                super::attention::label(k),
                super::attention::color(theme, k),
            )
        })
    });
    if let Some((label, color)) = badge {
        details = details.child(Node::text(
            label,
            Font::UiMedium,
            theme.font_size_small,
            color,
        ));
    }
    let background = if selected || dragged {
        theme.selected
    } else {
        Color::TRANSPARENT
    };
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 6.0,
            right: 4.0,
            bottom: 6.0,
            left: 12.0,
        })
        .gap(4.0)
        .align(Align::Center)
        .radius(theme.radius)
        .background(background)
        .hover_background(if selected {
            theme.selected
        } else {
            theme.hover
        })
        .key(key(&["thread", &thread.id]))
        .on_click(Msg::SelectThread(thread.id.clone()))
        .on_secondary_click(Msg::OpenMenu(MenuTarget::Thread(thread.id.clone())))
        .on_drag(Msg::DragThread(thread.id.clone()))
        .role(
            Role::Item,
            format!(
                "Thread {}{}{}",
                thread.title,
                if thread.pinned { ", pinned" } else { "" },
                attention.map_or(String::new(), |k| format!(
                    ", {}",
                    super::attention::label(k)
                )),
            ),
        )
        .child(details)
        .child(icon_button(
            theme,
            "×",
            &format!("Delete thread {}", thread.title),
            Msg::RequestDeleteThread(thread.id.clone()),
        ))
}

/// A thread's title, with a dot when its agent finished or waits (ADR 0017).
fn thread_title(
    theme: &Theme,
    thread: &v1::Thread,
    selected: bool,
    attention: Option<v1::AttentionKind>,
) -> Node<Msg> {
    let color = if selected {
        theme.text
    } else {
        theme.text.mix(theme.text_muted, 0.3)
    };
    Node::row()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center)
        .child(
            Node::text(&thread.title, Font::UiMedium, theme.font_size, color)
                .width(Length::Fill(1.0)),
        )
        .children(attention.map(|k| super::attention::indicator(theme, k, super::attention::DOT)))
}

/// Which repo and branch a thread is on. Pinned threads leave their status section, so
/// they show the status here.
fn thread_meta(state: &AppState, thread: &v1::Thread) -> String {
    let repo = state
        .workspace
        .repo(&thread.project_id)
        .map_or("unknown repo", |r| r.name.as_str());
    let mut meta = format!("{repo} · {}", thread.primary_branch);
    if thread.pinned && !thread.status.is_empty() {
        meta = format!("{} · {meta}", thread.status);
    }
    if thread.pinned {
        meta = format!("Pinned · {meta}");
    }
    meta
}
