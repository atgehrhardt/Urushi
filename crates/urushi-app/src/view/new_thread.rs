//! The new-thread dialog: repo, project and start-from branch are chosen from lists (the
//! daemon reports the repo's branches), title and branch name are typed. The branch name
//! follows the title until edited (see `update/intake.rs`). Opened from a board task, it
//! can instead start the task in an existing thread (`task_in_thread.rs`).

use std::collections::HashMap;

use urushi_ui::widgets::{ButtonKind, SelectOption, button, select_button, select_options};
use urushi_ui::{Key, Length, Node, Theme, key};

use super::dialogs::{cancel, field, hint, label};
use crate::msg::Msg;
use crate::organize::Select;
use crate::state::{AppState, Dialog, Field, TaskLink, ThreadTarget};
use crate::view::modal::modal;

/// Rows a selector shows before it scrolls.
const VISIBLE_OPTIONS: usize = 6;

/// The project choice that leaves the thread ungrouped.
const NO_PROJECT: &str = "None";

pub(super) fn view(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
) -> Option<Node<Msg>> {
    let Some(Dialog::NewThread {
        repo,
        project,
        title,
        branch,
        base,
        open,
        task,
        target,
        ..
    }) = &state.dialog
    else {
        return None;
    };
    let repo_name = state
        .workspace
        .repo(repo)
        .map_or("Choose a repo", |r| r.name.as_str());
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .children(for_task(state, theme, task.as_ref()));
    if task.is_some() && state.daemon_supports_task_in_thread() {
        let existing = matches!(target, ThreadTarget::Existing(_));
        body = body.child(super::task_in_thread::switch(theme, existing));
    }
    if let ThreadTarget::Existing(picked) = target {
        return Some(super::task_in_thread::dialog(
            state,
            theme,
            scroll,
            body,
            picked.as_deref(),
        ));
    }
    body = body
        .child(label(theme, "Repo"))
        .children(repo_selector(state, theme, scroll, repo, repo_name, *open))
        .children(project_selector(
            state,
            theme,
            scroll,
            project.as_deref(),
            *open,
        ))
        .child(label(theme, "Title"))
        .child(field(
            state,
            theme,
            title,
            Field::ThreadTitle,
            "What is this thread for?",
        ))
        .child(label(theme, "Branch"))
        .child(field(
            state,
            theme,
            branch,
            Field::ThreadBranch,
            "Generated from the title",
        ))
        .child(label(theme, "Start from"))
        .children(base_selector(
            state,
            theme,
            scroll,
            (repo, project.as_deref()),
            base.as_deref(),
            *open,
        ));
    body = body.child(hint(
        theme,
        "A new branch off “Start from”, in its own worktree, so agents never collide.",
    ));
    if task.is_some() {
        body = body.children(task_agent_hint(state, theme));
    }
    let heading = match project.as_ref().and_then(|p| state.workspace.project(p)) {
        Some(p) => format!("New thread in {}", p.name),
        None => "New thread".to_owned(),
    };
    Some(modal(
        state,
        theme,
        &heading,
        480.0,
        body,
        vec![
            cancel(theme),
            button(theme, "Create thread", ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    ))
}

/// Which task the dialog is for, if any.
fn for_task(state: &AppState, theme: &Theme, task: Option<&TaskLink>) -> Option<Node<Msg>> {
    let link = task?;
    let (_, _, t) = state.workspace.task(&link.task)?;
    let text = match &link.subtask {
        Some(s) => format!(
            "For subtask “{}” of “{}”",
            t.subtasks.iter().find(|x| x.id == *s)?.title,
            t.title
        ),
        None => format!("For task “{}”", t.title),
    };
    Some(hint(theme, &text))
}

/// What a thread started from a task will run: the task agent with the task as its
/// prompt, or (none chosen) the launcher.
fn task_agent_hint(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    if !state.daemon_supports_task_agent() {
        return None;
    }
    Some(match state.task_agent() {
        Some((agent, mode)) => hint(
            theme,
            &format!(
                "Starts {} {}.",
                crate::task_agent::describe(agent, mode),
                state.task_message_phrase()
            ),
        ),
        None => hint(
            theme,
            "No agent is set for tasks (Settings), so it opens on the launcher.",
        ),
    })
}

/// A selector's open option list, scrolling past [`VISIBLE_OPTIONS`] rows.
pub(super) fn options_list(
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    name: &str,
    options: Vec<SelectOption<Msg>>,
) -> Node<Msg> {
    let list_key = key(&["select-list", name]);
    select_options(
        theme,
        options,
        (list_key, scroll.get(&list_key).copied().unwrap_or(0.0)),
        VISIBLE_OPTIONS,
    )
}

/// The repo selector, and its options while open.
fn repo_selector(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    repo: &str,
    repo_name: &str,
    open: Option<Select>,
) -> Vec<Node<Msg>> {
    let is_open = open == Some(Select::Repo);
    let mut out = vec![select_button(
        theme,
        repo_name,
        is_open,
        key(&["select", "repo"]),
        Msg::ToggleSelect(Select::Repo),
    )];
    if is_open {
        let options = state
            .workspace
            .repos()
            .iter()
            .map(|r| SelectOption {
                label: r.name.clone(),
                selected: r.id == repo,
                on_pick: Msg::PickRepo(r.id.clone()),
            })
            .collect();
        out.push(options_list(theme, scroll, "repo", options));
    }
    out
}

/// The project selector (labelled), and its options while open: "None" leaves the
/// thread ungrouped. Hidden while there are no projects to choose.
fn project_selector(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    project: Option<&str>,
    open: Option<Select>,
) -> Vec<Node<Msg>> {
    let projects = state.workspace.projects();
    if projects.is_empty() {
        return Vec::new();
    }
    let current = project
        .and_then(|p| state.workspace.project(p))
        .map_or(NO_PROJECT, |p| p.name.as_str());
    let is_open = open == Some(Select::Project);
    let mut out = vec![
        label(theme, "Project"),
        select_button(
            theme,
            current,
            is_open,
            key(&["select", "project"]),
            Msg::ToggleSelect(Select::Project),
        ),
    ];
    if is_open {
        let none = SelectOption {
            label: NO_PROJECT.to_owned(),
            selected: project.is_none(),
            on_pick: Msg::PickProject(None),
        };
        let options = std::iter::once(none)
            .chain(projects.iter().map(|p| SelectOption {
                label: p.name.clone(),
                selected: project == Some(p.id.as_str()),
                on_pick: Msg::PickProject(Some(p.id.clone())),
            }))
            .collect();
        out.push(options_list(theme, scroll, "project", options));
    }
    out
}

/// The branch a thread of `project` in `repo` starts from when none is picked: the
/// project's parent branch, if it is in this repo (the daemon decides; this is only the
/// label).
fn parent_branch<'a>(state: &'a AppState, repo: &str, project: Option<&str>) -> Option<&'a str> {
    let settings = state.workspace.project(project?)?.settings.as_ref()?;
    (settings.base_repo_id == repo && !settings.base_branch.is_empty())
        .then_some(settings.base_branch.as_str())
}

/// The start-from selector: the default (the project's parent branch, or the repo's
/// current branch), then the repo's other local branches and its remote branches, as the
/// daemon reported them.
fn base_selector(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    (repo, project): (&str, Option<&str>),
    base: Option<&str>,
    open: Option<Select>,
) -> Vec<Node<Msg>> {
    let branches = state
        .branches
        .as_ref()
        .filter(|(r, _)| r == repo)
        .map(|(_, b)| b);
    let parent = parent_branch(state, repo, project);
    let current = parent.or_else(|| {
        branches
            .map(|b| b.current.as_str())
            .filter(|c| !c.is_empty())
    });
    let default_label = match (parent, current) {
        (Some(p), _) => format!("{p} (project)"),
        (None, Some(c)) => format!("{c} (current)"),
        (None, None) => "Repo default".to_owned(),
    };
    let is_open = open == Some(Select::Base);
    let mut out = vec![select_button(
        theme,
        base.unwrap_or(&default_label),
        is_open,
        key(&["select", "base"]),
        Msg::ToggleSelect(Select::Base),
    )];
    if !is_open {
        return out;
    }
    let mut options = vec![SelectOption {
        label: default_label.clone(),
        selected: base.is_none(),
        on_pick: Msg::PickBase(None),
    }];
    match branches {
        // The default branch is the first choice above.
        Some(b) => options.extend(
            b.branches
                .iter()
                .chain(&b.remote_branches)
                .filter(|n| Some(n.as_str()) != current)
                .map(|name| SelectOption {
                    label: name.clone(),
                    selected: base == Some(name.as_str()),
                    on_pick: Msg::PickBase(Some(name.clone())),
                }),
        ),
        None => options.push(SelectOption {
            label: "Loading branches…".to_owned(),
            selected: false,
            on_pick: Msg::ToggleSelect(Select::Base),
        }),
    }
    out.push(options_list(theme, scroll, "base", options));
    out
}
