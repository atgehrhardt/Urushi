//! Dialogs: add repo, the deletion warning, settings, connect, and one-line prompts.
//! The new-thread dialog is in `new_thread.rs`, the launcher in `launcher.rs`, the task
//! dialogs in `tasks.rs`.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, modal, text_area, text_field, toggle};
use urushi_ui::{Color, Font, Insets, Length, Node, Role, TextField, Theme, key};

use crate::msg::Msg;
use crate::state::{AppState, Dialog, Field, SettingToggle};

pub(super) fn view(
    state: &AppState,
    theme: &Theme,
    scroll: &std::collections::HashMap<urushi_ui::Key, f32>,
) -> Option<Node<Msg>> {
    Some(match state.dialog.as_ref()? {
        Dialog::AddProject { path } => add_project(state, theme, path),
        Dialog::NewThread { .. } => super::new_thread::view(state, theme, scroll)?,
        Dialog::Prompt {
            prompt,
            field: text,
        } => {
            let (title, placeholder, action) = prompt.texts();
            let body = field(state, theme, text, Field::Prompt, placeholder);
            modal(
                theme,
                title,
                420.0,
                body,
                vec![
                    cancel(theme),
                    button(theme, action, ButtonKind::Primary, Msg::Submit),
                ],
                Some(Msg::CloseDialog),
            )
        }
        Dialog::NewTab { highlighted } => super::launcher::dialog(state, theme, *highlighted),
        Dialog::DeleteThread {
            title,
            plan,
            deleting,
        } => delete_dialog(theme, title, plan, *deleting),
        Dialog::Settings { prefix, shell } => settings_dialog(
            state,
            theme,
            (
                field(state, theme, prefix, Field::BranchPrefix, "urushi/"),
                field(state, theme, shell, Field::DefaultShell, "Login shell"),
            ),
        ),
        Dialog::Connect { host } => connect(state, theme, host),
        Dialog::RestartDaemon {
            terminals,
            threads,
            restarting,
        } => super::daemon::confirm(theme, *terminals, *threads, *restarting),
        Dialog::Task {
            board,
            column,
            task,
            title,
            notes,
            subtasks,
            labels,
            nav,
        } => super::tasks::editor(
            state,
            (theme, scroll),
            (board, column, task.as_deref(), nav),
            (title, notes, subtasks, labels),
        ),
        Dialog::DeleteBoard { board, name, tasks } => {
            let jira = state
                .workspace
                .board(board)
                .is_some_and(|b| b.jira.is_some());
            super::board::delete_board(theme, name, *tasks, jira)
        }
        Dialog::ConnectJira {
            site,
            email,
            token,
            busy,
            error,
        } => super::jira::connect(
            state,
            theme,
            (site, email, token),
            (*busy, error.as_deref()),
        ),
        Dialog::ImportJira {
            query,
            boards,
            error,
        } => super::jira::import(
            state,
            (theme, scroll),
            query,
            (boards.as_deref(), error.as_deref()),
        ),
        Dialog::NestTask { task, query } => {
            super::tasks::nest_picker(state, (theme, scroll), task, query)
        }
        Dialog::DeleteColumn {
            name, tasks, into, ..
        } => super::board::delete_column(theme, name, *tasks, into),
        Dialog::Setup { step, agent, mode } => {
            super::setup::wizard(state, theme, *step, *agent, *mode)
        }
    })
}

pub(super) fn field(
    state: &AppState,
    theme: &Theme,
    f: &TextField,
    which: Field,
    placeholder: &str,
) -> Node<Msg> {
    text_field(
        theme,
        f,
        state.focus == Some(which),
        field_key(which),
        placeholder,
        Msg::FocusField(which),
    )
    .on_secondary_click(Msg::field_menu(which))
}

/// A dialog text field for several lines, `lines` tall (the field must be multiline).
pub(super) fn area(
    state: &AppState,
    theme: &Theme,
    f: &TextField,
    which: Field,
    (placeholder, lines): (&str, u8),
) -> Node<Msg> {
    text_area(
        theme,
        f,
        state.focus == Some(which),
        (field_key(which), placeholder, Msg::FocusField(which)),
        lines,
    )
    .on_secondary_click(Msg::field_menu(which))
}

/// The key of a dialog field's widget (the runner finds fields by it while selecting).
pub(crate) fn field_key(which: Field) -> urushi_ui::Key {
    key(&["field", &format!("{which:?}")])
}

pub(super) fn label(theme: &Theme, text: &str) -> Node<Msg> {
    Node::text(
        text,
        Font::UiMedium,
        theme.font_size_small,
        theme.text_muted,
    )
}

pub(super) fn hint(theme: &Theme, text: &str) -> Node<Msg> {
    Node::text(text, Font::Ui, theme.font_size_small, theme.text_muted)
}

pub(super) fn cancel(theme: &Theme) -> Node<Msg> {
    button(theme, "Cancel", ButtonKind::Secondary, Msg::CloseDialog)
}

/// One way to add a project: a row with a title and a line of detail. `None` for
/// `on_click` shows it as not available yet.
fn source(theme: &Theme, title: &str, detail: &str, on_click: Option<Msg>) -> Node<Msg> {
    let available = on_click.is_some();
    let mut row = Node::column()
        .width(Length::Fill(1.0))
        .padding(Insets::symmetric(12.0, 14.0))
        .gap(3.0)
        .radius(theme.radius)
        .background(theme.surface)
        .border(1.0, theme.border)
        .key(key(&["project-source", title]))
        .child(Node::text(
            title,
            Font::UiMedium,
            theme.font_size,
            if available {
                theme.text
            } else {
                theme.text_muted
            },
        ))
        .child(hint(theme, detail));
    if let Some(msg) = on_click {
        row = row
            .hover_background(theme.hover)
            .on_click(msg)
            .role(Role::Button, title);
    } else {
        row = row.role(Role::Label, format!("{title} (coming soon)"));
    }
    row
}

/// Adding a project: choose a folder with the system picker. Where the picker can't see
/// the daemon's files (a remote daemon, mobile), type its path instead.
fn add_project(state: &AppState, theme: &Theme, path: &TextField) -> Node<Msg> {
    let mut body = Node::column().width(Length::Fill(1.0)).gap(10.0);
    let mut actions = vec![cancel(theme)];
    if state.can_pick_folders() {
        body = body.child(source(
            theme,
            "Choose a folder…",
            "Any folder inside a git repository on this computer.",
            Some(Msg::BrowseFolder),
        ));
    } else {
        body = body
            .child(label(theme, "Repository on the connected machine"))
            .child(field(
                state,
                theme,
                path,
                Field::ProjectPath,
                "/path/to/repository",
            ));
        actions.push(button(theme, "Add", ButtonKind::Primary, Msg::Submit));
    }
    body = body
        .child(source(
            theme,
            "Remote over SSH — coming soon",
            "Work in a repository on another machine.",
            None,
        ))
        .child(source(
            theme,
            "Connect from your phone — coming soon",
            "Pair the mobile app with this workspace.",
            None,
        ));
    modal(
        theme,
        "Add repo",
        480.0,
        body,
        actions,
        Some(Msg::CloseDialog),
    )
}

fn connect(state: &AppState, theme: &Theme, host: &TextField) -> Node<Msg> {
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Host"))
        .child(field(
            state,
            theme,
            host,
            Field::Host,
            "machine.tailnet.ts.net",
        ))
        .child(hint(
            theme,
            "The MagicDNS name (or tailnet IP) of a machine serving Urushi on your tailnet.",
        ));
    let dismiss = (state.target != crate::config::ConnectTarget::Ask).then_some(Msg::CloseDialog);
    let mut actions = Vec::new();
    if dismiss.is_some() {
        actions.push(cancel(theme));
    }
    actions.push(button(theme, "Connect", ButtonKind::Primary, Msg::Submit));
    modal(theme, "Connect to a host", 460.0, body, actions, dismiss)
}

/// The deletion warning. It lists exactly what the daemon will destroy, and the default
/// action (Cancel, and Enter) never deletes.
fn delete_dialog(theme: &Theme, title: &str, plan: &v1::DeletionPlan, deleting: bool) -> Node<Msg> {
    let row = |what: &str, detail: String, color: Color| {
        Node::row()
            .width(Length::Fill(1.0))
            .gap(12.0)
            .child(
                Node::text(what, Font::UiMedium, theme.font_size, theme.text_muted)
                    .width(Length::Fixed(130.0)),
            )
            .child(Node::text(detail, Font::Ui, theme.font_size, color).width(Length::Fill(1.0)))
    };
    let mut body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(Node::text(
            "This permanently deletes:",
            Font::Ui,
            theme.font_size,
            theme.text,
        ))
        .child(row("Worktree", plan.worktree_path.clone(), theme.text));
    if plan.modified_files + plan.untracked_files > 0 {
        let detail = format!(
            "{} modified, {} untracked file(s) will be lost",
            plan.modified_files, plan.untracked_files
        );
        body = body.child(row("Uncommitted", detail, theme.warning));
    }
    if plan.unpushed_commits > 0 {
        body = body.child(row(
            "Unpushed",
            format!("{} commit(s) exist nowhere else", plan.unpushed_commits),
            theme.warning,
        ));
    }
    for branch in &plan.local_branches {
        body = body.child(row("Local branch", branch.clone(), theme.text));
    }
    for remote in &plan.remote_branches {
        body = body.child(row(
            "Remote branch",
            format!("{}/{}", remote.remote, remote.branch),
            theme.danger,
        ));
    }
    if !plan.delete_remote_branches {
        body = body.child(row(
            "Remote",
            "Remote branches are kept (see Settings)".into(),
            theme.text_muted,
        ));
    }
    if plan.running_panes > 0 {
        body = body.child(row(
            "Terminals",
            format!("{} running terminal(s) will be closed", plan.running_panes),
            theme.text,
        ));
    }
    for warning in &plan.warnings {
        body = body.child(row("Note", warning.clone(), theme.text_muted));
    }
    let mut delete = button(
        theme,
        if deleting {
            "Deleting…"
        } else {
            "Delete thread"
        },
        ButtonKind::Danger,
        Msg::ConfirmDelete,
    );
    if deleting {
        delete.on_click = None;
    }
    let card_body = Node::column()
        .width(Length::Fill(1.0))
        .padding(Insets::all(0.0))
        .child(body)
        .role(Role::Label, "Deletion details");
    modal(
        theme,
        &format!("Delete “{title}”?"),
        600.0,
        card_body,
        vec![
            button(theme, "Cancel", ButtonKind::Secondary, Msg::CloseDialog),
            delete,
        ],
        Some(Msg::CloseDialog),
    )
}

fn settings_dialog(
    state: &AppState,
    theme: &Theme,
    (prefix, shell): (Node<Msg>, Node<Msg>),
) -> Node<Msg> {
    let s = state.settings.clone().unwrap_or_default();
    let tailnet = state.tailnet.clone().unwrap_or_default();
    let status = if tailnet.serving {
        format!(
            "Serving at {}:{} — only {} can connect",
            tailnet.dns_name,
            tailnet.port,
            if tailnet.login.is_empty() {
                "you"
            } else {
                &tailnet.login
            }
        )
    } else if tailnet.running {
        format!("Tailscale is running as {}", tailnet.login)
    } else if !tailnet.error.is_empty() {
        tailnet.error.clone()
    } else {
        "Tailscale status unknown".to_owned()
    };
    let mut body = Node::column().width(Length::Fill(1.0)).gap(10.0);
    body = body.children(super::daemon::settings_section(state, theme));
    let body = body
        .child(toggle(
            theme,
            s.delete_remote_branches,
            "Delete remote branches when deleting a thread",
            Msg::Toggle(SettingToggle::DeleteRemoteBranches),
        ))
        .child(toggle(
            theme,
            tailnet.serving,
            "Serve on my tailnet",
            Msg::Toggle(SettingToggle::ServeTailnet),
        ))
        .child(
            Node::text(status, Font::Ui, theme.font_size_small, theme.text_muted).padding(Insets {
                top: 0.0,
                right: 0.0,
                bottom: 6.0,
                left: 42.0,
            }),
        )
        .child(label(theme, "Agents"))
        .children(urushi_core::Agent::ALL.into_iter().map(|agent| {
            let mut switch = toggle(
                theme,
                state.skips_permissions(agent),
                &format!(
                    "{agent}: skip permission prompts ({})",
                    agent.skip_permissions_flag()
                ),
                Msg::Toggle(SettingToggle::SkipPermissions(agent)),
            );
            // An older daemon can't store this setting; don't offer a switch that
            // silently snaps back.
            if !state.daemon_supports_agents() {
                switch.on_click = None;
                switch.cursor = urushi_ui::CursorIcon::Default;
            }
            switch
        }))
        .child(hint(
            theme,
            "Off by default. When on, the agent runs commands and edits files without asking.",
        ))
        .children(super::setup::settings_rows(state, theme))
        .children(super::jira::settings_rows(state, theme))
        .child(label(theme, "Branch prefix"))
        .child(prefix)
        .child(label(theme, "Default shell"))
        .child(shell);
    modal(
        theme,
        "Settings",
        620.0,
        body,
        vec![
            button(theme, "Close", ButtonKind::Secondary, Msg::CloseDialog),
            button(theme, "Save", ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}
