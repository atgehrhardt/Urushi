//! Jira in the app: signing in, importing a board, syncing it, opening issues, and an
//! editor that leaves what Jira owns to Jira; task kinds on any board.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::{attached, bodies};
use crate::msg::{BoardMsg, Effect, Followup, JiraMsg, Msg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Dialog, Field, FieldCommand};

fn answer(s: &mut AppState, followup: Followup, outcome: Outcome) -> Vec<Effect> {
    s.update(Msg::Response {
        followup,
        result: Ok(outcome),
    })
}

fn signed_in() -> v1::JiraAccount {
    v1::JiraAccount {
        connected: true,
        site: "https://acme.atlassian.net".into(),
        email: "a@b.c".into(),
        display_name: "Ada".into(),
    }
}

/// A Jira board: "ACME-1" (a story in epic ACME-9) with a subtask, in To Do.
pub(super) fn jira_board() -> v1::Board {
    let issue = |key: &str, epic: &str| {
        Some(Box::new(v1::JiraIssue {
            key: key.into(),
            url: format!("https://acme.atlassian.net/browse/{key}"),
            type_name: "Story".into(),
            status: "To Do".into(),
            assignee: "Ada Lovelace".into(),
            epic_key: epic.into(),
            notes_editable: true,
            ..Default::default()
        }))
    };
    v1::Board {
        id: "jb".into(),
        name: "ACME board".into(),
        columns: vec![
            v1::BoardColumn {
                id: "todo".into(),
                name: "To Do".into(),
                tasks: vec![v1::Task {
                    id: "j1".into(),
                    title: "Checkout".into(),
                    kind: v1::TaskKind::Story.into(),
                    issue: issue("ACME-1", "ACME-9"),
                    subtasks: vec![v1::Subtask {
                        id: "j2".into(),
                        title: "Tests".into(),
                        kind: v1::TaskKind::Subtask.into(),
                        issue: issue("ACME-2", ""),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                jira_statuses: vec!["1".into()],
                ..Default::default()
            },
            v1::BoardColumn {
                id: "done".into(),
                name: "Done".into(),
                jira_statuses: vec!["3".into()],
                ..Default::default()
            },
        ],
        jira: Some(Box::new(v1::JiraBoardSource {
            site: "https://acme.atlassian.net".into(),
            board_id: 7,
            name: "ACME board".into(),
            url: "https://acme.atlassian.net/secure/RapidBoard.jspa?rapidView=7".into(),
            synced_at_ms: 1,
            ..Default::default()
        })),
        epics: vec![v1::Epic {
            key: "ACME-9".into(),
            summary: "Billing".into(),
            color: v1::LabelColor::Purple.into(),
            url: String::new(),
        }],
        ..Default::default()
    }
}

/// A 1.12 daemon with the Jira board and an ordinary one, the panel open.
fn with_jira() -> AppState {
    let mut s = attached();
    s.daemon_minor = 12;
    let mut workspace = super::workspace();
    workspace.boards = vec![jira_board(), super::board::board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s
}

fn type_into(s: &mut AppState, field: Field, text: &str) {
    s.update(Msg::FocusField(field));
    s.update(Msg::Paste(text.into()));
}

#[test]
fn a_new_daemon_is_asked_who_it_is_signed_in_as() {
    let mut s = AppState::new(crate::config::ConnectTarget::Local(None), super::CELL);
    let fx = s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_minor: 12,
            ..Default::default()
        },
        workspace: super::workspace(),
    });
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::GetJiraAccount(_)))
    );
    answer(
        &mut s,
        Followup::JiraAccount,
        Outcome::JiraAccount(signed_in()),
    );
    assert_eq!(
        s.jira.as_ref().map(|a| a.display_name.as_str()),
        Some("Ada")
    );
    // An older daemon isn't asked.
    let fx = super::connected().update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: super::workspace(),
    });
    assert!(
        !bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::GetJiraAccount(_)))
    );
}

#[test]
fn importing_signs_in_first_then_lists_and_imports_a_board() {
    let mut s = with_jira();
    s.update(Msg::Jira(JiraMsg::OpenImport));
    assert!(
        matches!(s.dialog, Some(Dialog::ConnectJira { .. })),
        "not signed in yet"
    );
    assert_eq!(s.focus, Some(Field::JiraSite));

    // Everything is needed.
    assert!(bodies(&s.update(Msg::Submit)).is_empty());
    let Some(Dialog::ConnectJira { error, .. }) = &s.dialog else {
        panic!("still signing in");
    };
    assert!(error.is_some());

    type_into(&mut s, Field::JiraSite, "acme");
    type_into(&mut s, Field::JiraEmail, "a@b.c");
    type_into(&mut s, Field::JiraToken, "t0ken");
    // The token is secret: copying it gives nothing.
    s.update(Msg::EditField(FieldCommand::SelectAll));
    assert!(
        !s.update(Msg::EditField(FieldCommand::Copy))
            .iter()
            .any(|e| matches!(e, Effect::SetClipboard(_)))
    );
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::ConnectJira(v1::ConnectJiraRequest { site, email, api_token })]
            if site == "acme" && email == "a@b.c" && api_token == "t0ken"),
        "{fx:?}"
    );
    // A refusal shows in the dialog, not the banner.
    s.update(Msg::Response {
        followup: Followup::JiraConnected,
        result: Err("Jira didn't accept the email and API token".into()),
    });
    assert!(s.error.is_none());
    let Some(Dialog::ConnectJira { busy, error, .. }) = &s.dialog else {
        panic!("still signing in");
    };
    assert!(
        !busy
            && error
                .as_deref()
                .is_some_and(|e| e.contains("didn't accept"))
    );

    s.update(Msg::Submit);
    let fx = answer(
        &mut s,
        Followup::JiraConnected,
        Outcome::JiraAccount(signed_in()),
    );
    assert!(matches!(
        s.dialog,
        Some(Dialog::ImportJira { boards: None, .. })
    ));
    assert!(matches!(bodies(&fx)[..], [Body::ListJiraBoards(_)]));

    answer(
        &mut s,
        Followup::JiraBoards,
        Outcome::JiraBoards(v1::JiraBoardList {
            boards: vec![v1::JiraBoardInfo {
                id: 8,
                name: "Mobile".into(),
                ..Default::default()
            }],
        }),
    );
    assert!(matches!(&s.dialog, Some(Dialog::ImportJira { boards: Some(b), .. }) if b.len() == 1));
    // Enter searches again.
    type_into(&mut s, Field::JiraQuery, "mob");
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::ListJiraBoards(v1::ListJiraBoardsRequest { query })] if query == "mob")
    );

    let fx = s.update(Msg::Jira(JiraMsg::Import(8)));
    assert!(matches!(
        fx[..],
        [Effect::Request {
            body: Body::ImportJiraBoard(v1::ImportJiraBoardRequest { jira_board_id: 8 }),
            followup: Followup::BoardCreated,
        }]
    ));
    answer(
        &mut s,
        Followup::BoardCreated,
        Outcome::Board(v1::Board {
            id: "new".into(),
            ..Default::default()
        }),
    );
    assert!(s.dialog.is_none());
    assert_eq!(s.board.selected.as_deref(), Some("new"));
}

#[test]
fn boards_sync_and_issues_open_in_jira() {
    let mut s = with_jira();
    let fx = s.update(Msg::Jira(JiraMsg::Sync("jb".into())));
    assert!(
        matches!(bodies(&fx)[..], [Body::SyncBoard(v1::SyncBoardRequest { board_id })] if board_id == "jb")
    );
    let url = "https://acme.atlassian.net/browse/ACME-1";
    let fx = s.update(Msg::Jira(JiraMsg::Open(url.into())));
    assert!(matches!(&fx[..], [Effect::OpenUrl(u)] if u == url));
    // Signing out from Settings.
    s.jira = Some(signed_in());
    let fx = s.update(Msg::Jira(JiraMsg::Disconnect));
    assert!(matches!(bodies(&fx)[..], [Body::DisconnectJira(_)]));
    assert!(!s.jira.as_ref().is_some_and(|a| a.connected));
}

#[test]
fn the_epic_filter_narrows_a_jira_board_and_resets_with_another_board() {
    let mut s = with_jira();
    s.update(Msg::Board(BoardMsg::FilterEpic(Some("ACME-9".into()))));
    let board = jira_board();
    let card = &board.columns[0].tasks[0];
    assert!(crate::board::passes(&s.board, card));
    s.update(Msg::Board(BoardMsg::FilterEpic(Some("ACME-5".into()))));
    assert!(!crate::board::passes(&s.board, card));
    assert_eq!(crate::board::board_epics(&board).len(), 1);
    s.update(Msg::SelectBoard("b".into()));
    assert!(s.board.epic_filter.is_none());
}

#[test]
fn saving_a_jira_card_never_sends_its_type() {
    let mut s = with_jira();
    s.update(Msg::PressTask("j1".into()));
    s.update(Msg::PressEnded);
    let Some(Dialog::Task { labels, .. }) = &s.dialog else {
        panic!("editor open");
    };
    assert_eq!(labels.kind, i32::from(v1::TaskKind::Story));
    s.update(Msg::Board(BoardMsg::PickKind(v1::TaskKind::Bug.into())));
    type_into(&mut s, Field::TaskTitle, " v2");
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::UpdateTask(v1::UpdateTaskRequest { title: Some(t), kind: None, .. })] if t == "Checkout v2"),
        "{fx:?}"
    );
}

#[test]
fn tasks_get_a_kind_on_any_board() {
    let mut s = with_jira();
    s.update(Msg::SelectBoard("b".into()));
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    type_into(&mut s, Field::TaskTitle, "Crash on start");
    s.update(Msg::Board(BoardMsg::PickKind(v1::TaskKind::Bug.into())));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateTask(v1::CreateTaskRequest { kind, .. })] if *kind == i32::from(v1::TaskKind::Bug))
    );
    // Changing an existing task's kind is saved; picking it again clears it.
    s.update(Msg::PressTask("a".into()));
    s.update(Msg::PressEnded);
    s.update(Msg::Board(BoardMsg::PickKind(v1::TaskKind::Story.into())));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::UpdateTask(v1::UpdateTaskRequest { kind: Some(k), .. })] if *k == i32::from(v1::TaskKind::Story))
    );
}

#[test]
fn the_new_board_button_offers_an_import_on_a_jira_daemon() {
    let mut s = with_jira();
    s.update(Msg::OpenMenu(MenuTarget::NewBoard));
    assert!(matches!(
        s.menu.as_ref().map(|m| &m.target),
        Some(MenuTarget::NewBoard)
    ));
    s.jira = Some(signed_in());
    let fx = s.update(Msg::Jira(JiraMsg::OpenImport));
    assert!(s.menu.is_none());
    assert!(
        matches!(s.dialog, Some(Dialog::ImportJira { .. })),
        "signed in: straight to the list"
    );
    assert!(matches!(bodies(&fx)[..], [Body::ListJiraBoards(_)]));
}

#[test]
fn a_new_jira_task_is_created_only_on_save() {
    let mut s = with_jira();
    s.update(Msg::NewTask {
        board: "jb".into(),
        column: "todo".into(),
    });
    s.update(Msg::Text("Half typed".into()));
    // Enter in the title goes on to the notes: no issue is created in Jira yet.
    let fx = s.update(Msg::Key(super::named(crate::keys::Named::Enter)));
    assert!(fx.is_empty(), "{fx:?}");
    assert_eq!(s.focus, Some(Field::TaskNotes));
    // Closing asks before throwing it away; keep editing, then save.
    s.update(Msg::CloseDialog);
    assert!(matches!(&s.dialog, Some(Dialog::Task { nav, .. }) if nav.confirm_discard));
    s.update(Msg::Board(BoardMsg::KeepEditing));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateTask(v1::CreateTaskRequest { title, .. })] if title == "Half typed"),
        "{fx:?}"
    );
}
