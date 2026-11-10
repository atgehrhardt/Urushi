//! Round trips and wire messages for Jira boards.
#![allow(clippy::unwrap_used, reason = "test code")]

use prost::Message;
use urushi_core::board::{JiraColumn, SyncedIssue};
use urushi_core::{Board, BoardId, ColumnId, TaskId, Timestamp};

use super::*;
use crate::convert::{board_from_proto, board_to_proto};
use crate::v1::{request::Body, response::Outcome};

#[test]
fn every_kind_round_trips_and_unknown_values_are_none() {
    for k in TaskKind::ALL {
        assert_eq!(kind_from_proto(kind_to_proto(Some(k))), Some(k));
    }
    assert_eq!(kind_from_proto(kind_to_proto(None)), None);
    assert_eq!(kind_from_proto(99), None);
}

#[test]
fn issues_and_sources_round_trip() {
    let issue = Issue {
        id: "10001".into(),
        key: "ACME-1".into(),
        type_name: "Bug".into(),
        status_id: "3".into(),
        status: "In Progress".into(),
        category: StatusCategory::InProgress,
        assignee: Some("Ada".into()),
        priority: None,
        epic: Some("ACME-9".into()),
        parent: None,
        rank: "0|a".into(),
        updated: urushi_core::Timestamp::from_unix_millis(5),
        notes_editable: false,
        type_id: "10004".into(),
        assignee_id: Some("acc-ada".into()),
        reporter: Some(Person {
            account_id: "acc-grace".into(),
            name: "Grace".into(),
        }),
        priority_id: None,
        due: Some("2026-12-01".into()),
        points: Some("3".into()),
        sprint: Some(Named {
            id: "12".into(),
            name: "Sprint 4".into(),
        }),
        components: vec![Named {
            id: "1".into(),
            name: "API".into(),
        }],
        fix_versions: vec![Named {
            id: "2".into(),
            name: "1.0".into(),
        }],
        links: 2,
    };
    let wire = issue_to_proto(&issue, Some("https://acme.atlassian.net"));
    assert_eq!(wire.url, "https://acme.atlassian.net/browse/ACME-1");
    assert_eq!(issue_from_proto(&wire), issue);

    let source = BoardSource::Jira(JiraBoard {
        site: "https://acme.atlassian.net".into(),
        board_id: 7,
        name: "ACME".into(),
        project_key: "ACME".into(),
        scrum: true,
        rank_field: None,
        points_field: None,
        sprint_field: None,
        quick_filters: vec![QuickFilter {
            id: 4,
            name: "Only my issues".into(),
            // The daemon's: not sent.
            jql: String::new(),
        }],
        sync: SyncState {
            synced_at: Some(urushi_core::Timestamp::from_unix_millis(9)),
            error: Some("offline".into()),
            dropped: 2,
        },
    });
    let wire = source_to_proto(Some(&source));
    assert_eq!(source_from_proto(wire.as_ref()), Some(source));
    assert_eq!(source_to_proto(None), None);
}

#[test]
fn jira_boards_round_trip_with_issue_addresses() {
    let jira = JiraBoard {
        site: "https://acme.atlassian.net".into(),
        board_id: 7,
        name: "ACME".into(),
        project_key: "ACME".into(),
        scrum: false,
        rank_field: None,
        points_field: None,
        sprint_field: None,
        quick_filters: Vec::new(),
        sync: SyncState::default(),
    };
    let mut board = Board::new_jira(
        BoardId::from_u128(1),
        jira,
        &[JiraColumn {
            name: "To Do".into(),
            statuses: vec!["1".into()],
        }],
        Timestamp::from_unix_millis(1),
        || ColumnId::from_u128(2),
    )
    .unwrap();
    let issue = Issue {
        id: "10001".into(),
        key: "ACME-1".into(),
        status_id: "1".into(),
        epic: Some("ACME-9".into()),
        notes_editable: true,
        ..Issue::default()
    };
    board.merge_issues(
        vec![SyncedIssue {
            issue,
            title: "Fix it".into(),
            notes: String::new(),
            labels: vec!["ui".into()],
            kind: TaskKind::Bug,
            epic_summary: Some("Billing".into()),
            created: Timestamp::from_unix_millis(3),
        }],
        true,
        || TaskId::from_u128(5),
    );
    let wire = board_to_proto(&board);
    let task = &wire.columns[0].tasks[0];
    assert_eq!(task.kind, i32::from(v1::TaskKind::Bug));
    let issue = task.issue.as_ref().unwrap();
    assert_eq!(issue.url, "https://acme.atlassian.net/browse/ACME-1");
    assert_eq!(
        wire.epics[0].url,
        "https://acme.atlassian.net/browse/ACME-9"
    );
    assert_eq!(wire.columns[0].jira_statuses, ["1"]);
    assert_eq!(board_from_proto(&wire).unwrap(), board);
}

#[test]
fn jira_messages_round_trip_on_the_wire() {
    let bodies = [
        Body::ConnectJira(v1::ConnectJiraRequest {
            site: "acme".into(),
            email: "a@b.c".into(),
            api_token: "t".into(),
        }),
        Body::GetJiraAccount(v1::GetJiraAccountRequest {}),
        Body::DisconnectJira(v1::DisconnectJiraRequest {}),
        Body::ListJiraBoards(v1::ListJiraBoardsRequest { query: "q".into() }),
        Body::ImportJiraBoard(v1::ImportJiraBoardRequest { jira_board_id: 7 }),
        Body::SyncBoard(v1::SyncBoardRequest {
            board_id: "1".into(),
        }),
        Body::QueryJiraFilter(v1::QueryJiraFilterRequest {
            board_id: "1".into(),
            quick_filter_ids: vec![4, 5],
        }),
    ];
    for body in bodies {
        let request = v1::Request {
            id: 3,
            body: Some(body),
        };
        let decoded = v1::Request::decode(request.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, request);
    }
    let outcomes = [
        Outcome::JiraAccount(v1::JiraAccount {
            connected: true,
            site: "https://acme.atlassian.net".into(),
            email: "a@b.c".into(),
            display_name: "Ada".into(),
            account_id: "acc-ada".into(),
        }),
        Outcome::JiraIssueIds(v1::JiraIssueIds {
            ids: vec!["10001".into()],
        }),
        Outcome::JiraBoards(v1::JiraBoardList {
            boards: vec![v1::JiraBoardInfo {
                id: 7,
                name: "ACME".into(),
                scrum: true,
                project_key: "ACME".into(),
                project_name: "Acme".into(),
                imported_board_id: "1".into(),
            }],
        }),
    ];
    for outcome in outcomes {
        let response = v1::Response {
            id: 3,
            outcome: Some(outcome),
        };
        let decoded = v1::Response::decode(response.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, response);
    }
}
