//! The project-settings dialog (ADR 0029): the parent branch (a repo and one of its
//! local or remote branches, as the daemon reports them) and the switch for the
//! parent-branch thread pinned at the project's top.

use std::collections::HashMap;

use urushi_ui::widgets::{ButtonKind, SelectOption, button, modal, select_button, toggle};
use urushi_ui::{Key, Length, Node, Theme, key};

use super::dialogs::{cancel, hint, label};
use super::new_thread::options_list;
use crate::msg::Msg;
use crate::organize::Select;
use crate::project_settings::{ProjectSettingsDraft, ProjectSettingsMsg as M};
use crate::state::AppState;

/// The parent-branch repo choice that means "no parent branch".
const NO_PARENT: &str = "None";

pub(super) fn view(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    draft: &ProjectSettingsDraft,
) -> Node<Msg> {
    let name = state
        .workspace
        .project(&draft.project)
        .map_or("Project", |p| p.name.as_str());
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Repo"))
        .children(repo_selector(state, theme, scroll, draft));
    if let Some(repo) = &draft.repo {
        body = body
            .child(label(theme, "Parent branch"))
            .children(branch_selector(state, theme, scroll, draft, repo));
    }
    body = body
        .child(hint(
            theme,
            "New threads of this project start from the parent branch.",
        ))
        .child(toggle(
            theme,
            draft.base_thread,
            "Keep a thread on the parent branch at the top of the project",
            Msg::ProjectSettings(M::ToggleBaseThread),
        ))
        .child(hint(theme, base_thread_hint(state, draft)));
    modal(
        theme,
        &format!("{name} settings"),
        480.0,
        body,
        vec![
            cancel(theme),
            button(theme, "Save", ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}

/// What the switch does, given the thread that is open now.
fn base_thread_hint(state: &AppState, draft: &ProjectSettingsDraft) -> &'static str {
    let open = state.workspace.project_base_thread(&draft.project);
    match (open, draft.base_thread) {
        (Some(thread), _) if draft.needs_closing(thread, remotes(state, draft)) => {
            "Saving closes the open parent-branch thread first (you'll confirm)."
        }
        (Some(_), _) => "It runs in the checkout that has the branch.",
        (None, true) => {
            "It runs where the branch is already checked out (often the repo's own \
             folder), or in a worktree of its own. Its branch is never deleted."
        }
        (None, false) => "No parent-branch thread.",
    }
}

fn remotes<'a>(state: &'a AppState, draft: &ProjectSettingsDraft) -> &'a [String] {
    state
        .branches
        .as_ref()
        .filter(|(r, _)| Some(r) == draft.repo.as_ref())
        .map_or(&[], |(_, b)| b.remote_branches.as_slice())
}

fn repo_selector(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    draft: &ProjectSettingsDraft,
) -> Vec<Node<Msg>> {
    let current = draft
        .repo
        .as_ref()
        .and_then(|r| state.workspace.repo(r))
        .map_or(NO_PARENT, |r| r.name.as_str());
    let is_open = draft.open == Some(Select::Repo);
    let mut out = vec![select_button(
        theme,
        current,
        is_open,
        key(&["select", "parent-repo"]),
        Msg::ProjectSettings(M::ToggleSelect(Select::Repo)),
    )];
    if is_open {
        let options = std::iter::once(SelectOption {
            label: NO_PARENT.to_owned(),
            selected: draft.repo.is_none(),
            on_pick: Msg::ProjectSettings(M::PickRepo(None)),
        })
        .chain(state.workspace.repos().iter().map(|r| SelectOption {
            label: r.name.clone(),
            selected: draft.repo.as_deref() == Some(r.id.as_str()),
            on_pick: Msg::ProjectSettings(M::PickRepo(Some(r.id.clone()))),
        }))
        .collect();
        out.push(options_list(theme, scroll, "parent-repo", options));
    }
    out
}

/// The repo's local branches, then its remote ones, as the daemon reported them.
fn branch_selector(
    state: &AppState,
    theme: &Theme,
    scroll: &HashMap<Key, f32>,
    draft: &ProjectSettingsDraft,
    repo: &str,
) -> Vec<Node<Msg>> {
    let is_open = draft.open == Some(Select::Base);
    let mut out = vec![select_button(
        theme,
        draft.branch.as_deref().unwrap_or("Choose a branch"),
        is_open,
        key(&["select", "parent-branch"]),
        Msg::ProjectSettings(M::ToggleSelect(Select::Base)),
    )];
    if !is_open {
        return out;
    }
    let branches = state
        .branches
        .as_ref()
        .filter(|(r, _)| r == repo)
        .map(|(_, b)| b);
    let options = match branches {
        Some(b) => b
            .branches
            .iter()
            .chain(&b.remote_branches)
            .map(|name| SelectOption {
                label: name.clone(),
                selected: draft.branch.as_deref() == Some(name.as_str()),
                on_pick: Msg::ProjectSettings(M::PickBranch(name.clone())),
            })
            .collect(),
        None => vec![SelectOption {
            label: "Loading branches…".to_owned(),
            selected: false,
            on_pick: Msg::ProjectSettings(M::ToggleSelect(Select::Base)),
        }],
    };
    out.push(options_list(theme, scroll, "parent-branch", options));
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use urushi_client::v1;
    use urushi_ui::{CellMetrics, Point, Rect, TextSystem, Theme, build};

    use crate::config::ConnectTarget;
    use crate::msg::Msg;
    use crate::organize::{MenuTarget, PROJECT_SETTINGS_SINCE_MINOR};
    use crate::state::AppState;

    /// Project "Billing" with a pinned thread `pin` and its parent-branch thread `m`
    /// (listed after it, to show the sidebar puts it first anyway).
    fn state(minor: u32) -> AppState {
        let mut state = AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        );
        let thread = |id: &str, title: &str, branch: &str| v1::Thread {
            id: id.into(),
            project_id: "r".into(),
            title: title.into(),
            primary_branch: branch.into(),
            folder_id: "f".into(),
            state: v1::ThreadState::Ready.into(),
            ..Default::default()
        };
        state.update(Msg::Connected {
            welcome: v1::Welcome::default(),
            workspace: v1::Workspace {
                projects: vec![v1::Project {
                    id: "r".into(),
                    name: "api".into(),
                    ..Default::default()
                }],
                folders: vec![v1::Folder {
                    id: "f".into(),
                    name: "Billing".into(),
                    settings: Some(v1::FolderSettings {
                        base_repo_id: "r".into(),
                        base_branch: "main".into(),
                        base_thread: Some(true),
                    }),
                    ..Default::default()
                }],
                threads: vec![
                    v1::Thread {
                        pinned: true,
                        ..thread("pin", "Invoices", "urushi/invoices")
                    },
                    v1::Thread {
                        project_base: true,
                        attached_checkout: true,
                        ..thread("m", "main", "main")
                    },
                ],
                ..Default::default()
            },
        });
        state.daemon_minor = minor;
        state
    }

    fn render(state: &AppState) -> urushi_ui::Frame<Msg> {
        let mut text = TextSystem::bundled_only();
        build(
            &crate::view::view(state, &Theme::dark(), &HashMap::new()),
            Rect::new(0.0, 0.0, 1200.0, 800.0),
            &mut text,
            None::<Point>,
        )
    }

    fn clicks(frame: &urushi_ui::Frame<Msg>) -> Vec<Msg> {
        frame
            .hits
            .iter()
            .filter_map(|h| h.on_click.clone())
            .collect()
    }

    #[test]
    fn the_parent_branch_thread_is_first_and_cannot_be_dragged_or_deleted_from_its_card() {
        let frame = render(&state(PROJECT_SETTINGS_SINCE_MINOR));
        let dump = frame.scene.dump();
        let parent = dump.find("Parent branch · api · main").expect("its card");
        let pinned = dump
            .find("Pinned · api · urushi/invoices")
            .expect("the pinned one");
        assert!(parent < pinned, "{dump}");
        let clicks = clicks(&frame);
        assert!(
            clicks
                .iter()
                .any(|m| matches!(m, Msg::RequestDeleteThread(t) if t == "pin"))
        );
        assert!(
            !clicks
                .iter()
                .any(|m| matches!(m, Msg::RequestDeleteThread(t) if t == "m"))
        );
        assert!(
            !frame
                .hits
                .iter()
                .any(|h| matches!(&h.on_drag, Some(Msg::DragThread(t)) if t == "m"))
        );
    }

    #[test]
    fn its_menu_only_renames_or_closes_it() {
        let mut state = state(PROJECT_SETTINGS_SINCE_MINOR);
        state.update(Msg::OpenMenu(MenuTarget::Thread("m".into())));
        let dump = render(&state).scene.dump();
        assert!(dump.contains("Close parent-branch thread…"), "{dump}");
        assert!(!dump.contains("\"Pin\"") && !dump.contains("Move to project"));
    }

    #[test]
    fn project_settings_are_offered_by_new_daemons_and_show_the_parent_branch() {
        let mut old = state(PROJECT_SETTINGS_SINCE_MINOR - 1);
        old.update(Msg::OpenMenu(MenuTarget::Project("f".into())));
        assert!(!render(&old).scene.dump().contains("Project settings…"));

        let mut state = state(PROJECT_SETTINGS_SINCE_MINOR);
        state.update(Msg::OpenMenu(MenuTarget::Project("f".into())));
        let frame = render(&state);
        assert!(
            clicks(&frame)
                .iter()
                .any(|m| matches!(m, Msg::OpenProjectSettings(p) if p == "f"))
        );
        state.update(Msg::OpenProjectSettings("f".into()));
        let dump = render(&state).scene.dump();
        assert!(dump.contains("Billing settings"), "{dump}");
        assert!(dump.contains("\"main\""), "{dump}");
        assert!(dump.contains("It runs in the checkout that has the branch."));
    }
}
