//! Dialogs: add project, new thread, the deletion warning, settings, connect.

use urushi_client::v1;
use urushi_ui::widgets::{ButtonKind, button, modal, text_field, toggle};
use urushi_ui::{Color, Font, Insets, Length, Node, Role, TextField, Theme, key};

use crate::msg::Msg;
use crate::state::{AppState, Dialog, Field, SettingToggle};

pub(super) fn view(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    Some(match state.dialog.as_ref()? {
        Dialog::AddProject { path } => add_project(state, theme, path),
        Dialog::NewThread {
            project,
            title,
            base,
        } => new_thread(state, theme, project, (title, base)),
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
    })
}

fn field(
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
        key(&["field", &format!("{which:?}")]),
        placeholder,
        Msg::FocusField(which),
    )
}

fn label(theme: &Theme, text: &str) -> Node<Msg> {
    Node::text(
        text,
        Font::UiMedium,
        theme.font_size_small,
        theme.text_muted,
    )
}

fn hint(theme: &Theme, text: &str) -> Node<Msg> {
    Node::text(text, Font::Ui, theme.font_size_small, theme.text_muted)
}

fn cancel(theme: &Theme) -> Node<Msg> {
    button(theme, "Cancel", ButtonKind::Secondary, Msg::CloseDialog)
}

fn add_project(state: &AppState, theme: &Theme, path: &TextField) -> Node<Msg> {
    let mut row = Node::row().width(Length::Fill(1.0)).gap(8.0).child(field(
        state,
        theme,
        path,
        Field::ProjectPath,
        "/path/to/repository",
    ));
    if cfg!(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    )) && is_local(state)
    {
        row = row.child(button(
            theme,
            "Browse…",
            ButtonKind::Secondary,
            Msg::BrowseFolder,
        ));
    }
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Repository"))
        .child(row)
        .child(hint(
            theme,
            "Any folder inside a git repository, on the machine the daemon runs on.",
        ));
    modal(
        theme,
        "Add project",
        520.0,
        body,
        vec![
            cancel(theme),
            button(theme, "Add", ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}

fn new_thread(
    state: &AppState,
    theme: &Theme,
    project: &str,
    (title, base): (&TextField, &TextField),
) -> Node<Msg> {
    let project_name = state
        .workspace
        .project(project)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(8.0)
        .child(label(theme, "Title"))
        .child(field(
            state,
            theme,
            title,
            Field::ThreadTitle,
            "What is this thread for?",
        ))
        .child(label(theme, "Start from"))
        .child(field(
            state,
            theme,
            base,
            Field::ThreadBase,
            "Current branch",
        ))
        .child(hint(
            theme,
            "Every thread gets its own branch and git worktree, so agents never collide.",
        ));
    let heading = format!("New thread in {project_name}");
    let actions = vec![
        cancel(theme),
        button(theme, "Create thread", ButtonKind::Primary, Msg::Submit),
    ];
    modal(
        theme,
        &heading,
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

fn is_local(state: &AppState) -> bool {
    matches!(state.target, crate::config::ConnectTarget::Local(_))
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
    let body = Node::column()
        .width(Length::Fill(1.0))
        .gap(10.0)
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
        .child(label(theme, "Branch prefix"))
        .child(prefix)
        .child(label(theme, "Default shell"))
        .child(shell);
    modal(
        theme,
        "Settings",
        520.0,
        body,
        vec![
            button(theme, "Close", ButtonKind::Secondary, Msg::CloseDialog),
            button(theme, "Save", ButtonKind::Primary, Msg::Submit),
        ],
        Some(Msg::CloseDialog),
    )
}
