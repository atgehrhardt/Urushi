//! The launcher, the new-thread branch name, adding projects, and dropped files.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{Agent, RepoId, ThreadId};

use super::{CELL, attached, bodies, connected, id, key, named, workspace};
use crate::config::ConnectTarget;
use crate::keys::{Mods, Named};
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Dialog, Launch, SettingToggle};

fn thread_id() -> String {
    ThreadId::from_u128(10).to_string()
}

/// Connected to a daemon that speaks protocol 1.1, with Claude installed.
fn with_agents() -> AppState {
    let mut s = AppState::new(ConnectTarget::Local(None), CELL);
    let fx = s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_major: 1,
            protocol_minor: 1,
            ..Default::default()
        },
        workspace: workspace(),
    });
    assert!(
        bodies(&fx).iter().any(|b| matches!(b, Body::ListAgents(_))),
        "agents are requested from a 1.1 daemon"
    );
    s.update(Msg::Response {
        followup: Followup::Agents,
        result: Ok(Outcome::Agents(v1::Agents {
            agents: vec![v1::InstalledAgent {
                agent: v1::Agent::Claude.into(),
                path: "/usr/local/bin/claude".into(),
            }],
        })),
    });
    s
}

/// The same workspace after its only tab was closed.
fn without_tabs(s: &mut AppState) {
    let mut w = workspace();
    w.tabs.clear();
    w.threads[0].tab_ids.clear();
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
}

fn create_tab_agent(fx: &[Effect]) -> Option<v1::Agent> {
    bodies(fx).iter().find_map(|b| match b {
        Body::CreateTab(r) => Some(r.agent()),
        _ => None,
    })
}

#[test]
fn old_daemons_are_not_asked_for_agents_and_offer_only_a_terminal() {
    let s = connected(); // protocol 1.0
    assert!(s.agents.is_empty());
    assert_eq!(s.launch_choices(), vec![Launch::Terminal]);

    let mut s = with_agents();
    assert_eq!(
        s.launch_choices(),
        vec![Launch::Terminal, Launch::Agent(Agent::Claude)]
    );
    // A failed lookup is not an error worth a banner.
    s.update(Msg::Response {
        followup: Followup::Agents,
        result: Err("unsupported".into()),
    });
    assert!(s.error.is_none());
}

#[test]
fn the_new_tab_button_opens_a_picker() {
    let mut s = with_agents();
    let fx = s.update(Msg::NewTab);
    assert!(matches!(s.dialog, Some(Dialog::NewTab { highlighted: 0 })));
    assert!(
        create_tab_agent(&fx).is_none(),
        "nothing opens until a choice is made"
    );

    // Arrow down + Enter picks the highlighted agent.
    s.update(Msg::Key(named(Named::Down)));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert_eq!(create_tab_agent(&fx), Some(v1::Agent::Claude));
    assert!(s.dialog.is_none());

    // A digit picks directly; clicking a choice does too.
    s.update(Msg::NewTab);
    let fx = s.update(Msg::Key(key("1", Mods::default())));
    assert_eq!(create_tab_agent(&fx), Some(v1::Agent::Unspecified));
    s.update(Msg::NewTab);
    let fx = s.update(Msg::Launch(Launch::Agent(Agent::Claude)));
    assert_eq!(create_tab_agent(&fx), Some(v1::Agent::Claude));

    // Escape cancels.
    s.update(Msg::NewTab);
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.dialog.is_none());
}

#[test]
fn closing_the_last_tab_leaves_the_launcher_not_a_dead_end() {
    let mut s = with_agents();
    without_tabs(&mut s);
    assert_eq!(
        s.selected_thread,
        Some(thread_id()),
        "the thread stays selected"
    );
    assert!(s.empty_thread().is_some());
    assert!(s.current_tab().is_none());

    // Typing doesn't go anywhere odd; digits and Enter launch.
    let fx = s.update(Msg::Key(key("2", Mods::default())));
    assert_eq!(create_tab_agent(&fx), Some(v1::Agent::Claude));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert_eq!(create_tab_agent(&fx), Some(v1::Agent::Unspecified));
    let fx = s.update(Msg::Key(key("x", Mods::default())));
    assert!(fx.is_empty());
}

#[test]
fn a_new_thread_opens_on_the_launcher() {
    let mut s = with_agents();
    s.update(Msg::OpenNewThread {
        repo: Some(RepoId::from_u128(1).to_string()),
        project: None,
    });
    let fx = s.update(Msg::Submit);
    assert!(matches!(
        bodies(&fx)[..],
        [Body::CreateThread(v1::CreateThreadRequest {
            skip_initial_tab: true,
            ..
        })]
    ));
    let fx = s.update(Msg::Response {
        followup: Followup::ThreadCreated(1),
        result: Ok(Outcome::Thread(v1::Thread {
            id: thread_id(),
            ..Default::default()
        })),
    });
    assert!(
        bodies(&fx).iter().any(|b| matches!(b, Body::ListAgents(_))),
        "the launcher's list is refreshed"
    );
}

#[test]
fn the_branch_follows_the_title_until_edited() {
    let mut s = attached();
    s.update(Msg::OpenNewThread {
        repo: Some(RepoId::from_u128(1).to_string()),
        project: None,
    });
    s.update(Msg::Paste("My New Thread".into()));
    let branch = |s: &AppState| match &s.dialog {
        Some(Dialog::NewThread { branch, title, .. }) => {
            (branch.text().to_owned(), title.text().to_owned())
        }
        _ => panic!("dialog closed"),
    };
    assert_eq!(branch(&s).0, "urushi/my-new-thread");

    // Editing the branch leaves the title alone, and stops the title driving it.
    s.update(Msg::Key(named(Named::Tab)));
    for _ in 0.."my-new-thread".len() {
        s.update(Msg::Key(named(Named::Backspace)));
    }
    s.update(Msg::Paste("Primary".into()));
    assert_eq!(
        branch(&s),
        ("urushi/Primary".into(), "My New Thread".into())
    );
    s.update(Msg::Key(named(Named::Tab))); // back to the title
    s.update(Msg::Key(key("!", Mods::default())));
    assert_eq!(
        branch(&s),
        ("urushi/Primary".into(), "My New Thread!".into())
    );

    // Clearing the branch makes it follow the title again.
    s.update(Msg::Key(named(Named::Tab)));
    for _ in 0.."urushi/Primary".len() {
        s.update(Msg::Key(named(Named::Backspace)));
    }
    s.update(Msg::Key(named(Named::Tab)));
    s.update(Msg::Key(named(Named::Backspace)));
    assert_eq!(branch(&s).0, "urushi/my-new-thread");
}

#[test]
fn adding_a_local_project_goes_straight_to_the_folder_picker() {
    let mut s = attached();
    s.update(Msg::OpenAddProject);
    if !s.can_pick_folders() {
        return; // mobile targets
    }
    assert!(s.focus.is_none(), "no path to type");
    assert!(matches!(&s.update(Msg::Submit)[..], [Effect::PickFolder]));
    let fx = s.update(Msg::FolderPicked(Some("/src/repo".into())));
    assert!(
        matches!(bodies(&fx)[..], [Body::AddProject(v1::AddProjectRequest { path, .. })] if path == "/src/repo")
    );
    // Cancelling the picker adds nothing.
    assert!(s.update(Msg::FolderPicked(None)).is_empty());
}

#[test]
fn a_remote_daemon_still_takes_a_typed_path() {
    let mut s = AppState::new(ConnectTarget::Remote("box".into()), CELL);
    s.update(Msg::OpenAddProject);
    assert!(s.focus.is_some());
    s.update(Msg::Paste("/srv/repo".into()));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::AddProject(v1::AddProjectRequest { path, .. })] if path == "/srv/repo")
    );
}

#[test]
fn dropped_files_are_pasted_into_the_terminal_and_folders_become_projects() {
    let mut s = attached();
    let fx = s.update(Msg::FileDropped {
        path: "/Users/me/Screen Shot.png".into(),
        is_dir: false,
        over_terminal: false,
    });
    assert!(
        matches!(&fx[..], [Effect::Notify(v1::notification::Body::Input(v1::Input { pane_id, kind: Some(v1::input::Kind::Paste(text)) }))] if *pane_id == id(100) && text == r"/Users/me/Screen\ Shot.png "),
        "{fx:?}"
    );

    let fx = s.update(Msg::FileDropped {
        path: "/src/repo".into(),
        is_dir: true,
        over_terminal: false,
    });
    assert!(matches!(bodies(&fx)[..], [Body::AddProject(_)]));

    // A folder dropped onto a terminal is a path for the terminal.
    let fx = s.update(Msg::FileDropped {
        path: "/src/repo".into(),
        is_dir: true,
        over_terminal: true,
    });
    assert!(matches!(&fx[..], [Effect::Notify(_)]));

    // Dialogs (other than add-project) swallow drops.
    s.update(Msg::OpenSettings);
    let fx = s.update(Msg::FileDropped {
        path: "/x.png".into(),
        is_dir: false,
        over_terminal: true,
    });
    assert!(fx.is_empty());
}

#[test]
fn updating_bases_from_their_remote_is_toggled() {
    let mut s = with_agents();
    s.settings = Some(v1::Settings {
        update_base_from_remote: true,
        ..Default::default()
    });
    let fx = s.update(Msg::Toggle(SettingToggle::UpdateBaseFromRemote));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateSettings(v1::UpdateSettingsRequest {
            patch: Some(v1::SettingsPatch {
                update_base_from_remote: Some(false),
                delete_remote_branches: None,
                ..
            })
        })]
    ));
    let fx = s.update(Msg::Toggle(SettingToggle::DeleteRemoteBranches));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateSettings(v1::UpdateSettingsRequest {
            patch: Some(v1::SettingsPatch {
                delete_remote_branches: Some(true),
                update_base_from_remote: None,
                ..
            })
        })]
    ));
}

#[test]
fn skip_permissions_is_toggled_per_agent() {
    let mut s = with_agents();
    s.settings = Some(v1::Settings {
        agents: Some(v1::AgentSettings {
            codex_skip_permissions: true,
            ..Default::default()
        }),
        ..Default::default()
    });
    let fx = s.update(Msg::Toggle(SettingToggle::SkipPermissions(Agent::Claude)));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateSettings(v1::UpdateSettingsRequest {
            patch: Some(v1::SettingsPatch {
                claude_skip_permissions: Some(true),
                codex_skip_permissions: None,
                ..
            })
        })]
    ));
    let fx = s.update(Msg::Toggle(SettingToggle::SkipPermissions(Agent::Codex)));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateSettings(v1::UpdateSettingsRequest {
            patch: Some(v1::SettingsPatch {
                codex_skip_permissions: Some(false),
                ..
            })
        })]
    ));
}
