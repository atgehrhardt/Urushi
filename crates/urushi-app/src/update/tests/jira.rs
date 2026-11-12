//! Jira in the app: signing in, importing a board, syncing it, opening issues, and an
//! editor that leaves what Jira owns to Jira; task kinds on any board.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::{attached, bodies};
use crate::msg::{BoardMsg, Effect, FilterMsg, Followup, JiraMsg, Msg};
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
        account_id: "acc-ada".into(),
        key: "acme.atlassian.net/a@b.c".into(),
    }
}

/// Signed in on a second site (1.27).
fn globex() -> v1::JiraAccount {
    v1::JiraAccount {
        connected: true,
        site: "https://globex.atlassian.net".into(),
        email: "a@b.c".into(),
        display_name: "Ada".into(),
        account_id: "acc-ada-globex".into(),
        key: "globex.atlassian.net/a@b.c".into(),
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
    assert_eq!(s.jira_accounts.len(), 1);
    assert_eq!(s.jira_accounts[0].display_name, "Ada");
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
        matches!(bodies(&fx)[..], [Body::ListJiraBoards(v1::ListJiraBoardsRequest { query, .. })] if query == "mob")
    );

    let fx = s.update(Msg::Jira(JiraMsg::Import(8)));
    assert!(matches!(
        fx[..],
        [Effect::Request {
            body: Body::ImportJiraBoard(v1::ImportJiraBoardRequest {
                jira_board_id: 8,
                ..
            }),
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
    s.jira_accounts = vec![signed_in()];
    let fx = s.update(Msg::Jira(JiraMsg::Disconnect(signed_in().key)));
    assert!(matches!(bodies(&fx)[..], [Body::DisconnectJira(_)]));
    assert!(s.jira_accounts.is_empty());
}

#[test]
fn filters_narrow_a_jira_board_and_reset_with_another_board() {
    use crate::filter::Dim;
    let mut s = with_jira();
    s.update(Msg::SelectBoard("jb".into()));
    // Picking values leaves the menu open, to pick more.
    s.update(Msg::OpenMenu(MenuTarget::Filter("jb".into())));
    s.update(Msg::Filter(FilterMsg::Toggle(Dim::Epic, "ACME-9".into())));
    assert!(s.menu.is_some());
    let board = jira_board();
    let card = &board.columns[0].tasks[0];
    let cx = s.filter_context();
    assert!(s.board.filter.shows(card, cx));
    s.update(Msg::Filter(FilterMsg::Toggle(Dim::Epic, "ACME-9".into())));
    s.update(Msg::Filter(FilterMsg::Toggle(Dim::Epic, "ACME-5".into())));
    assert!(!s.board.filter.shows(card, s.filter_context()));
    // "Only my issues" goes by the signed-in account.
    s.update(Msg::Filter(FilterMsg::Clear));
    s.jira_accounts = vec![signed_in()];
    s.update(Msg::Filter(FilterMsg::Mine));
    assert_eq!(s.filter_context().me, Some("acc-ada"));
    s.update(Msg::SelectBoard("b".into()));
    assert!(!s.board.filter.active());
}

#[test]
fn quick_filters_ask_jira_and_ask_again_when_the_board_changes() {
    let mut s = with_jira();
    s.daemon_minor = 13;
    s.update(Msg::SelectBoard("jb".into()));
    let fx = s.update(Msg::Filter(FilterMsg::Quick(4)));
    assert!(matches!(
        &fx[..],
        [Effect::Request { body: Body::QueryJiraFilter(v1::QueryJiraFilterRequest { board_id, quick_filter_ids, .. }), followup: Followup::QuickFilter(b, 1) }]
            if board_id == "jb" && quick_filter_ids == &[4] && b == "jb"
    ));
    // A change that leaves the board alone (an agent's attention) asks nothing.
    let unchanged = |s: &AppState| {
        let mut workspace = super::workspace();
        workspace.boards = s.workspace.raw().boards.clone();
        workspace
    };
    let same = unchanged(&s);
    assert!(bodies(&s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(same)))).is_empty());
    assert!(!s.board.quick_stale);
    // The board changes before Jira answers: asked again once the answer comes.
    let mut workspace = unchanged(&s);
    for b in &mut workspace.boards {
        if b.id == "jb" {
            b.columns[0].tasks[0].title = "Checkout, again".into();
        }
    }
    assert!(
        bodies(&s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)))).is_empty()
    );
    let fx = answer(
        &mut s,
        Followup::QuickFilter("jb".into(), 1),
        Outcome::JiraIssueIds(v1::JiraIssueIds {
            ids: vec!["nope".into()],
        }),
    );
    assert!(matches!(bodies(&fx)[..], [Body::QueryJiraFilter(_)]));
    let board = jira_board();
    assert!(
        !s.board
            .filter
            .shows(&board.columns[0].tasks[0], s.filter_context())
    );
    // Off again: nothing hidden, nothing asked.
    assert!(s.update(Msg::Filter(FilterMsg::Quick(4))).is_empty());
    assert!(
        s.board
            .filter
            .shows(&board.columns[0].tasks[0], s.filter_context())
    );
}

#[test]
fn turning_quick_filters_on_and_off_never_flashes_the_whole_board() {
    let mut s = with_jira();
    s.daemon_minor = 13;
    s.update(Msg::SelectBoard("jb".into()));
    let card = jira_board().columns[0].tasks[0].clone();
    s.update(Msg::Filter(FilterMsg::Quick(4)));
    answer(
        &mut s,
        Followup::QuickFilter("jb".into(), 1),
        Outcome::JiraIssueIds(v1::JiraIssueIds {
            ids: vec!["nope".into()],
        }),
    );
    assert!(!s.board.filter.shows(&card, s.filter_context()));
    // Another one on: until Jira answers, the board stays as it was, not unfiltered.
    s.update(Msg::Filter(FilterMsg::Quick(5)));
    assert!(s.board.quick_asking);
    assert!(!s.board.filter.shows(&card, s.filter_context()));
    // The answer to the first query, arriving late, is dropped.
    let issue = card
        .issue
        .as_ref()
        .map(|i| i.id.clone())
        .unwrap_or_default();
    answer(
        &mut s,
        Followup::QuickFilter("jb".into(), 1),
        Outcome::JiraIssueIds(v1::JiraIssueIds {
            ids: vec![issue.clone()],
        }),
    );
    assert!(!s.board.filter.shows(&card, s.filter_context()));
    assert!(s.board.quick_asking, "still waiting for the newer answer");
    answer(
        &mut s,
        Followup::QuickFilter("jb".into(), 2),
        Outcome::JiraIssueIds(v1::JiraIssueIds { ids: vec![issue] }),
    );
    assert!(s.board.filter.shows(&card, s.filter_context()));
    assert!(!s.board.quick_asking);
}

#[test]
fn the_search_takes_the_keyboard_and_escape_clears_it_first() {
    let mut s = with_jira();
    s.update(Msg::FocusField(Field::BoardSearch));
    s.update(Msg::Text("acme".into()));
    assert_eq!(s.board.filter.search.text(), "acme");
    s.update(Msg::Key(super::named(crate::keys::Named::Backspace)));
    assert_eq!(s.board.filter.search.text(), "acm");
    s.update(Msg::Key(super::named(crate::keys::Named::Escape)));
    assert!(s.board.filter.search.text().is_empty() && s.board.open);
    s.update(Msg::Key(super::named(crate::keys::Named::Escape)));
    assert_eq!(s.focus, None);
    assert!(s.board.open, "the panel stays until Escape again");
    s.update(Msg::Key(super::named(crate::keys::Named::Escape)));
    assert!(!s.board.open);
}

#[test]
fn a_thread_for_an_issue_suggests_a_branch_led_by_its_key() {
    let mut s = with_jira();
    for (link, wanted) in [
        (
            crate::state::TaskLink {
                task: "j1".into(),
                subtask: None,
            },
            "urushi/ACME-1-checkout",
        ),
        (
            crate::state::TaskLink {
                task: "j1".into(),
                subtask: Some("j2".into()),
            },
            "urushi/ACME-2-tests",
        ),
    ] {
        s.update(Msg::StartThread(link));
        let Some(Dialog::NewThread { branch, .. }) = &s.dialog else {
            panic!("{:?}", s.dialog);
        };
        assert_eq!(branch.text(), wanted);
    }
    // Retitled, it keeps the key.
    s.update(Msg::Text(" now".into()));
    let Some(Dialog::NewThread { branch, .. }) = &s.dialog else {
        panic!("{:?}", s.dialog);
    };
    assert_eq!(branch.text(), "urushi/ACME-2-tests-now");
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
    s.jira_accounts = vec![signed_in()];
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

#[test]
fn a_new_epic_is_created_on_save_from_a_daemon_that_makes_epics_cards() {
    let mut s = with_jira();
    assert!(
        !s.daemon_supports_epic_cards(),
        "a 1.12 daemon refuses epics"
    );
    s.daemon_minor = crate::board::EPIC_CARDS_SINCE_MINOR;
    assert!(s.daemon_supports_epic_cards());
    s.update(Msg::NewTask {
        board: "jb".into(),
        column: "todo".into(),
    });
    type_into(&mut s, Field::TaskTitle, "Payments");
    s.update(Msg::Board(BoardMsg::PickKind(v1::TaskKind::Epic.into())));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateTask(v1::CreateTaskRequest { title, kind, .. })]
            if title == "Payments" && *kind == i32::from(v1::TaskKind::Epic)),
        "{fx:?}"
    );
}

#[test]
fn an_epic_card_shows_under_its_own_epic_filter() {
    use crate::filter::{Dim, values};
    use urushi_client::TaskRef;
    let epic = v1::Task {
        id: "e".into(),
        kind: v1::TaskKind::Epic.into(),
        issue: Some(Box::new(v1::JiraIssue {
            key: "ACME-9".into(),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(
        values(Dim::Epic, TaskRef::Card(&epic)),
        [("ACME-9".to_owned(), "ACME-9".to_owned())]
    );
    let mut s = with_jira();
    s.update(Msg::SelectBoard("jb".into()));
    s.update(Msg::Filter(FilterMsg::Toggle(Dim::Epic, "ACME-9".into())));
    assert!(s.board.filter.shows(&epic, s.filter_context()));
}

/// The Jira board shown by a 1.13 daemon, which is told the panel shows it (so it
/// reads it every few seconds), and told again when that changes.
fn with_jira_13() -> AppState {
    let mut s = with_jira();
    s.daemon_minor = 13;
    let fx = s.update(Msg::SelectBoard("jb".into()));
    assert!(
        matches!(&bodies(&fx)[..], [Body::WatchBoard(r)] if r.board_id == "jb"),
        "{fx:?}"
    );
    assert!(
        s.update(Msg::SelectBoard("jb".into())).is_empty(),
        "told once"
    );
    let fx = s.update(Msg::SelectBoard("b".into()));
    assert!(matches!(&bodies(&fx)[..], [Body::WatchBoard(r)] if r.board_id.is_empty()));
    s.update(Msg::SelectBoard("jb".into()));
    s
}

/// The Jira board on a 1.13 daemon, the editor open on ACME-1, its issue answered.
fn editing_issue() -> AppState {
    let mut s = with_jira_13();
    s.jira_accounts = vec![signed_in()];
    let fx = s.update(Msg::Board(BoardMsg::OpenTask("j1".into())));
    assert!(
        matches!(&fx[..], [Effect::Request { body: Body::GetJiraIssue(r), followup: Followup::JiraIssue(t) }] if r.task_id == "j1" && t == "j1"),
        "{fx:?}"
    );
    let option = |id: &str, name: &str| v1::JiraOption {
        id: id.into(),
        name: name.into(),
    };
    let details = v1::JiraIssueDetails {
        task_id: "j1".into(),
        fields: vec![
            v1::JiraField {
                id: "priority".into(),
                name: "Priority".into(),
                kind: v1::JiraFieldKind::Option.into(),
                allowed: vec![option("1", "Highest"), option("2", "High")],
                value: Some(v1::JiraFieldValue {
                    text: String::new(),
                    options: vec![option("2", "High")],
                }),
                ..Default::default()
            },
            v1::JiraField {
                id: "duedate".into(),
                name: "Due date".into(),
                kind: v1::JiraFieldKind::Date.into(),
                ..Default::default()
            },
            v1::JiraField {
                id: "labels".into(),
                name: "Labels".into(),
                kind: v1::JiraFieldKind::Labels.into(),
                ..Default::default()
            },
        ],
        link_types: vec![v1::JiraLinkType {
            name: "Blocks".into(),
            inward: "is blocked by".into(),
            outward: "blocks".into(),
        }],
        ..Default::default()
    };
    answer(
        &mut s,
        Followup::JiraIssue("j1".into()),
        Outcome::JiraIssueDetails(details),
    );
    s
}

fn issue(s: &AppState) -> &crate::issue::IssueEditor {
    let Some(Dialog::Task { issue, .. }) = &s.dialog else {
        panic!("{:?}", s.dialog);
    };
    issue
}

#[test]
fn an_issues_fields_are_drafts_until_save() {
    use crate::msg::IssueMsg;
    let mut s = editing_issue();
    assert!(issue(&s).details.is_some());
    s.update(Msg::Issue(IssueMsg::Open(Some("priority".into()))));
    let fx = s.update(Msg::Issue(IssueMsg::Pick {
        field: "priority".into(),
        option: Some(v1::JiraOption {
            id: "1".into(),
            name: "Highest".into(),
        }),
    }));
    assert!(fx.is_empty(), "nothing reaches Jira before Save");
    assert!(issue(&s).open.is_none());
    // A date typed into its field, and a label added as typed.
    s.update(Msg::FocusField(Field::JiraText(1)));
    s.update(Msg::Text("2026-12-01".into()));
    s.update(Msg::Issue(IssueMsg::Open(Some("labels".into()))));
    s.update(Msg::Text("urgent".into()));
    assert!(
        s.update(Msg::Key(super::named(crate::keys::Named::Enter)))
            .is_empty()
    );
    let fx = s.update(Msg::Submit);
    let [
        Body::ChangeJiraIssue(v1::ChangeJiraIssueRequest {
            task_id,
            change: Some(v1::change_jira_issue_request::Change::Edit(edits)),
        }),
    ] = &bodies(&fx)[..]
    else {
        panic!("{fx:?}");
    };
    assert_eq!(task_id, "j1");
    let shown: Vec<(&str, String)> = edits
        .edits
        .iter()
        .map(|e| {
            let v = e.value.as_ref().unwrap();
            (
                e.field_id.as_str(),
                format!(
                    "{}{:?}",
                    v.text,
                    v.options.iter().map(|o| &o.name).collect::<Vec<_>>()
                ),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("labels", "[\"urgent\"]".to_owned()),
            ("priority", "[\"Highest\"]".to_owned()),
            ("duedate", "2026-12-01[]".to_owned()),
        ]
    );
}

#[test]
fn comments_links_work_and_files_go_to_jira_at_once() {
    use crate::msg::IssueMsg;
    use v1::change_jira_issue_request::Change;
    let mut s = editing_issue();
    let change = |fx: &[Effect]| match bodies(fx)[..] {
        [
            Body::ChangeJiraIssue(v1::ChangeJiraIssueRequest {
                change: Some(c), ..
            }),
        ] => c.clone(),
        _ => panic!("{fx:?}"),
    };
    s.update(Msg::FocusField(Field::JiraComment));
    s.update(Msg::Paste("Looks **good**".into()));
    let fx = s.update(Msg::Issue(IssueMsg::Comment));
    assert_eq!(change(&fx), Change::AddComment("Looks **good**".into()));
    assert!(issue(&s).comment.text().is_empty());
    s.update(Msg::Issue(IssueMsg::LinkKind(0, false)));
    let fx = s.update(Msg::Issue(IssueMsg::Link("ACME-2".into())));
    assert!(
        matches!(change(&fx), Change::Link(l) if l.link_type == "Blocks" && !l.outward && l.issue_key == "ACME-2")
    );
    s.update(Msg::FocusField(Field::JiraWork));
    s.update(Msg::Paste("1h 30m".into()));
    let fx = s.update(Msg::Key(super::named(crate::keys::Named::Enter)));
    assert!(matches!(change(&fx), Change::LogWork(w) if w.seconds == 5400));
    s.update(Msg::Paste("soon".into()));
    assert!(s.update(Msg::Issue(IssueMsg::LogWork)).is_empty());
    assert!(s.error.is_some(), "says how to type it");
    let fx = s.update(Msg::Board(BoardMsg::Attach {
        task: "j1".into(),
        name: "plan.txt".into(),
        bytes: b"x".to_vec(),
    }));
    assert!(matches!(change(&fx), Change::Attach(a) if a.name == "plan.txt"));
    assert!(
        matches!(&fx[0], Effect::Request { followup: Followup::JiraIssue(t), .. } if t == "j1")
    );
}

#[test]
fn a_new_jira_task_needs_its_required_fields() {
    let mut s = with_jira_13();
    let fx = s.update(Msg::NewTask {
        board: "jb".into(),
        column: "todo".into(),
    });
    assert!(
        matches!(bodies(&fx)[..], [Body::GetJiraCreateFields(_)]),
        "{fx:?}"
    );
    answer(
        &mut s,
        Followup::JiraCreateFields(v1::TaskKind::Task.into()),
        Outcome::JiraIssueDetails(v1::JiraIssueDetails {
            fields: vec![v1::JiraField {
                id: "customfield_1".into(),
                name: "Team".into(),
                kind: v1::JiraFieldKind::Option.into(),
                required: true,
                allowed: vec![v1::JiraOption {
                    id: "t1".into(),
                    name: "Payments".into(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        }),
    );
    s.update(Msg::Text("Ship".into()));
    assert!(s.update(Msg::Submit).is_empty());
    assert_eq!(s.error.as_deref(), Some("Fill in Team first."));
    s.update(Msg::Issue(crate::msg::IssueMsg::Pick {
        field: "customfield_1".into(),
        option: Some(v1::JiraOption {
            id: "t1".into(),
            name: "Payments".into(),
        }),
    }));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(&bodies(&fx)[..], [Body::CreateTask(r)] if r.jira_fields.len() == 1 && r.jira_fields[0].field_id == "customfield_1"),
        "{fx:?}"
    );
    // Another type asks again.
    s.update(Msg::NewTask {
        board: "jb".into(),
        column: "todo".into(),
    });
    let fx = s.update(Msg::Board(BoardMsg::PickKind(v1::TaskKind::Bug.into())));
    assert!(
        matches!(bodies(&fx)[..], [Body::GetJiraCreateFields(r)] if r.kind == i32::from(v1::TaskKind::Bug))
    );
}

#[test]
fn a_new_daemon_lists_every_sign_in() {
    let mut s = AppState::new(crate::config::ConnectTarget::Local(None), super::CELL);
    let fx = s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_minor: crate::board::JIRA_ACCOUNTS_SINCE_MINOR,
            ..Default::default()
        },
        workspace: super::workspace(),
    });
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::ListJiraAccounts(_)))
    );
    answer(
        &mut s,
        Followup::JiraAccounts,
        Outcome::JiraAccounts(v1::JiraAccountList {
            accounts: vec![signed_in(), globex()],
        }),
    );
    assert_eq!(s.jira_accounts, [signed_in(), globex()]);
}

#[test]
fn several_sign_ins_import_from_the_one_chosen_and_each_board_has_its_own_me() {
    let mut s = with_jira();
    s.daemon_minor = crate::board::JIRA_ACCOUNTS_SINCE_MINOR;
    s.jira_accounts = vec![signed_in(), globex()];
    // The import dialog lists the first site's boards, then the one picked.
    // (The panel's board is now watched too: a 1.13+ daemon.)
    let fx = s.update(Msg::Jira(JiraMsg::OpenImport));
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::ListJiraBoards(r) if r.account == signed_in().key)),
        "{fx:?}"
    );
    let fx = s.update(Msg::Jira(JiraMsg::ImportFrom(globex().key)));
    assert!(matches!(
        &bodies(&fx)[..],
        [Body::ListJiraBoards(r)] if r.account == globex().key
    ));
    let fx = s.update(Msg::Jira(JiraMsg::Import(8)));
    assert!(matches!(
        &bodies(&fx)[..],
        [Body::ImportJiraBoard(r)] if r.account == globex().key && r.jira_board_id == 8
    ));
    s.update(Msg::CloseDialog);

    // "Me" is the sign-in of the board on screen: by the key it names, else its site.
    s.update(Msg::SelectBoard("jb".into()));
    assert_eq!(s.filter_context().me, Some("acc-ada"));
    let mut workspace = super::workspace();
    let mut board = jira_board();
    if let Some(j) = board.jira.as_mut() {
        j.site = "https://globex.atlassian.net".into();
        j.account = globex().key;
    }
    workspace.boards = vec![board, super::board::board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    assert_eq!(s.filter_context().me, Some("acc-ada-globex"));

    // Signing out of one keeps the other; its board can't sync, and offers to sign in.
    let fx = s.update(Msg::Jira(JiraMsg::Disconnect(globex().key)));
    assert!(matches!(
        &bodies(&fx)[..],
        [Body::DisconnectJira(r)] if r.account == globex().key
    ));
    assert_eq!(s.jira_accounts, [signed_in()]);
    assert_eq!(s.filter_context().me, None);
    s.update(Msg::Jira(JiraMsg::Reconnect("jb".into())));
    let Some(Dialog::ConnectJira { site, email, .. }) = &s.dialog else {
        panic!("signing in again");
    };
    assert_eq!(
        (site.text(), email.text()),
        ("globex.atlassian.net", "a@b.c")
    );
    assert_eq!(s.focus, Some(Field::JiraToken));
    // Signed in again: added beside the other, not replacing it.
    answer(
        &mut s,
        Followup::JiraConnected,
        Outcome::JiraAccount(globex()),
    );
    assert_eq!(s.jira_accounts, [signed_in(), globex()]);
}

#[test]
fn an_older_daemon_has_one_sign_in_that_a_new_one_replaces() {
    let mut s = with_jira();
    s.jira_accounts = vec![signed_in()];
    s.update(Msg::Jira(JiraMsg::OpenConnect));
    answer(
        &mut s,
        Followup::JiraConnected,
        Outcome::JiraAccount(globex()),
    );
    assert_eq!(s.jira_accounts, [globex()]);
    // Its import dialog has no account picker, and asks for the daemon's one.
    let Some(Dialog::ImportJira { account, .. }) = &s.dialog else {
        panic!("importing");
    };
    assert_eq!(*account, globex().key);
}
