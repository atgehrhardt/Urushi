//! Round trips and wire messages for Jira boards.
#![allow(clippy::unwrap_used, reason = "test code")]

use prost::Message;
use urushi_core::board::{JiraColumn, PrStatus, PrUrl, PullRequest, SyncedIssue};
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
        pull_requests: vec![
            PullRequest::new(
                PrUrl::parse("https://github.com/acme/app/pull/3").unwrap(),
                "ACME-1 fix",
                PrStatus::Open,
            ),
            PullRequest::new(
                PrUrl::parse("https://github.com/acme/app/pull/2").unwrap(),
                "",
                PrStatus::Unknown,
            ),
        ],
    };
    let wire = issue_to_proto(&issue, Some("https://acme.atlassian.net"));
    assert_eq!(wire.url, "https://acme.atlassian.net/browse/ACME-1");
    assert_eq!(
        wire.pull_requests[0].status,
        v1::PullRequestStatus::Open as i32
    );
    assert_eq!(issue_from_proto(&wire), issue);
    // A pull request with an address a client couldn't open is left out.
    let mut bad = wire.clone();
    bad.pull_requests[0].url = "ftp://x/pull/3".into();
    assert_eq!(issue_from_proto(&bad).pull_requests.len(), 1);

    let source = BoardSource::Jira(JiraBoard {
        site: "https://acme.atlassian.net".into(),
        board_id: 7,
        name: "ACME".into(),
        project_key: "ACME".into(),
        scrum: true,
        rank_field: None,
        points_field: None,
        sprint_field: None,
        quick_filters: vec![JiraQuickFilter {
            id: 4,
            name: "Only my issues".into(),
            jql: "assignee = currentUser()".into(),
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
            kept_quick_filter_ids: vec!["k".into()],
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

#[test]
fn issue_editor_messages_round_trip() {
    use crate::v1::change_jira_issue_request::Change;
    let value = Some(v1::JiraFieldValue {
        text: "3".into(),
        options: vec![v1::JiraOption {
            id: "2".into(),
            name: "High".into(),
        }],
    });
    let changes = [
        Change::Edit(v1::JiraFieldEdits {
            edits: vec![v1::JiraFieldEdit {
                field_id: "priority".into(),
                value: value.clone(),
            }],
        }),
        Change::TransitionId("31".into()),
        Change::AddComment("**hi**".into()),
        Change::UpdateComment(v1::JiraCommentEdit {
            comment_id: "5".into(),
            body: "edited".into(),
        }),
        Change::DeleteCommentId("5".into()),
        Change::Attach(v1::JiraNewAttachment {
            name: "a.png".into(),
            data: vec![1, 2, 3],
        }),
        Change::DeleteAttachmentId("9".into()),
        Change::Link(v1::JiraNewLink {
            link_type: "Blocks".into(),
            issue_key: "A-2".into(),
            outward: true,
        }),
        Change::UnlinkId("77".into()),
        Change::Watch(true),
        Change::LogWork(v1::JiraNewWorklog {
            seconds: 3600,
            started_ms: 5,
            comment: "pairing".into(),
        }),
    ];
    let mut bodies: Vec<Body> = changes
        .into_iter()
        .map(|c| {
            Body::ChangeJiraIssue(v1::ChangeJiraIssueRequest {
                task_id: "t".into(),
                change: Some(c),
            })
        })
        .collect();
    bodies.extend([
        Body::GetJiraIssue(v1::GetJiraIssueRequest {
            task_id: "t".into(),
        }),
        Body::GetJiraCreateFields(v1::GetJiraCreateFieldsRequest {
            board_id: "b".into(),
            kind: v1::TaskKind::Story.into(),
        }),
        Body::GetJiraAttachment(v1::GetJiraAttachmentRequest {
            task_id: "t".into(),
            attachment_id: "9".into(),
            thumbnail: true,
        }),
        Body::SearchJiraUsers(v1::SearchJiraUsersRequest {
            task_id: "t".into(),
            query: "ad".into(),
            assignable: true,
        }),
        Body::SearchJiraIssues(v1::SearchJiraIssuesRequest {
            board_id: "b".into(),
            query: "A-2".into(),
        }),
    ]);
    for body in bodies {
        let request = v1::Request {
            id: 4,
            body: Some(body),
        };
        let decoded = v1::Request::decode(request.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, request);
    }
}

#[test]
fn issue_editor_answers_round_trip() {
    let user = v1::JiraUser {
        account_id: "acc-1".into(),
        name: "Ada".into(),
    };
    let value = Some(v1::JiraFieldValue {
        text: "3".into(),
        options: vec![v1::JiraOption {
            id: "2".into(),
            name: "High".into(),
        }],
    });
    let issue = v1::JiraIssueRef {
        id: "10002".into(),
        key: "A-2".into(),
        summary: "Other".into(),
        status: "To Do".into(),
        type_name: "Bug".into(),
        url: "https://x/browse/A-2".into(),
    };
    let outcomes = [
        Outcome::JiraIssueDetails(v1::JiraIssueDetails {
            task_id: "t".into(),
            fields: vec![v1::JiraField {
                id: "priority".into(),
                name: "Priority".into(),
                kind: v1::JiraFieldKind::Option.into(),
                required: true,
                allowed: vec![v1::JiraOption::default()],
                value,
            }],
            transitions: vec![v1::JiraTransition {
                id: "31".into(),
                name: "Done".into(),
                to_status: "10001".into(),
                to_category: v1::JiraStatusCategory::Done.into(),
            }],
            comments: vec![v1::JiraComment {
                id: "5".into(),
                author: Some(user.clone()),
                body: "hi".into(),
                created_ms: 1,
                updated_ms: 2,
                mine: true,
            }],
            attachments: vec![v1::JiraAttachment {
                id: "9".into(),
                name: "a.png".into(),
                media_type: "image/png".into(),
                size: 3,
                created_ms: 1,
                author: Some(user.clone()),
            }],
            links: vec![v1::JiraLink {
                id: "77".into(),
                relation: "blocks".into(),
                issue: Some(issue.clone()),
            }],
            link_types: vec![v1::JiraLinkType {
                name: "Blocks".into(),
                inward: "is blocked by".into(),
                outward: "blocks".into(),
            }],
            watchers: vec![user.clone()],
            watching: true,
            worklogs: vec![v1::JiraWorklog {
                id: "8".into(),
                author: Some(user.clone()),
                seconds: 60,
                started_ms: 1,
                comment: String::new(),
            }],
            original_estimate: "1d".into(),
            remaining_estimate: "4h".into(),
            time_spent: "4h".into(),
        }),
        Outcome::JiraUsers(v1::JiraUsers { users: vec![user] }),
        Outcome::JiraIssueRefs(v1::JiraIssueRefs {
            issues: vec![issue],
        }),
    ];
    for outcome in outcomes {
        let response = v1::Response {
            id: 4,
            outcome: Some(outcome),
        };
        let decoded = v1::Response::decode(response.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, response);
    }
}
