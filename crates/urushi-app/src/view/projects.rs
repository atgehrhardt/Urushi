//! The sidebar's Projects tab: each project (collapsible), then threads in no project.
//! Inside each group: pinned threads first, then one collapsible section per status in
//! use, then threads without a status. Threads can be dragged onto a project (or onto
//! "No project") to move them, or onto another thread to take its place and join its
//! group (`thread_card.rs`), and a project's header can be dragged onto another project
//! to take its place.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Color, Font, Insets, Length, Node, Role, Theme, key};

use crate::msg::Msg;
use crate::organize::{Dragged, DropTarget, MenuTarget, section_key};
use crate::state::{AppState, Connection};

use super::board::drop_line;
use super::thread_card::thread_card;

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
    let dragging_thread = matches!(state.dragging, Some((Dragged::Thread(_), _)));
    if !ungrouped.is_empty() || dragging_thread {
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
/// thread is dragged, the whole block is a drop target; while a project is dragged, each
/// project is. A project's header can be dragged (protocol 1.11).
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
    if let Some(project) = b.project
        && state.daemon_supports_project_order()
    {
        header = header.on_drag(Msg::DragProject(project.to_owned()));
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
        });
    if state.dragging.is_some() {
        column = column.background(Color::TRANSPARENT);
    }
    let drop = match (&state.dragging, b.project) {
        (Some((Dragged::Thread(_), _)), project) => Some(Msg::DropThread(
            project.map_or(DropTarget::Ungrouped, |p| DropTarget::Project(p.to_owned())),
        )),
        (Some((Dragged::Project(dragged), _)), Some(project)) => {
            if dragged == project {
                column = column.background(theme.selected);
            }
            Some(Msg::DropProject(project.to_owned()))
        }
        _ => None,
    };
    if let Some(drop) = drop {
        column = column
            .hover_background(theme.accent.with_alpha(0.15))
            .border(1.0, theme.border)
            .on_drop(drop);
    }
    let (line_above, line_below) = drop_line_at(state, b.project);
    let content = if collapsed {
        Vec::new()
    } else if b.threads.is_empty() {
        vec![hint(theme, "Drag threads here, or use + for a new one.")]
    } else {
        group(state, theme, b.project, &b.threads)
    };
    column
        .children(line_above.then(|| drop_line(theme)))
        .child(header)
        .children(content)
        .children(line_below.then(|| drop_line(theme)))
}

/// While a project is dragged over `project`: whether the line showing where it would
/// land goes above this block (moving up) or below it (moving down).
fn drop_line_at(state: &AppState, project: Option<&str>) -> (bool, bool) {
    let (Some(project), Some((Dragged::Project(dragged), _))) = (project, &state.dragging) else {
        return (false, false);
    };
    if state.project_over.as_deref() != Some(project) {
        return (false, false);
    }
    let projects = state.workspace.projects();
    let at = |id: &str| projects.iter().position(|p| p.id == id);
    match (at(dragged), at(project)) {
        (Some(from), Some(to)) => (to < from, to > from),
        _ => (false, false),
    }
}

/// The project's parent-branch thread (ADR 0029), then pinned threads, then a section
/// per status, then threads without a status.
fn group(
    state: &AppState,
    theme: &Theme,
    project: Option<&str>,
    threads: &[&v1::Thread],
) -> Vec<Node<Msg>> {
    // Each thread's Jira issue and pull request (ADR 0036), read in one pass.
    let links = state.workspace.thread_links();
    let thread_card = |t: &v1::Thread| thread_card(state, theme, t, links.get(&t.id));
    let mut out: Vec<Node<Msg>> = threads
        .iter()
        .filter(|t| t.project_base)
        .chain(threads.iter().filter(|t| t.pinned && !t.project_base))
        .map(|t| thread_card(t))
        .collect();
    let rest: Vec<&v1::Thread> = threads
        .iter()
        .copied()
        .filter(|t| !t.pinned && !t.project_base)
        .collect();
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
            out.extend(members.iter().map(|t| thread_card(t)));
        }
    }
    out.extend(
        rest.iter()
            .filter(|t| t.status.is_empty())
            .map(|t| thread_card(t)),
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
