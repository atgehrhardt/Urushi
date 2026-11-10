//! Jira boards and task kinds: core ⇄ wire (since 1.12). Unknown enum values from a
//! newer peer read as unspecified; an issue's web address is filled in from its board's
//! site, so it travels without being stored.

use urushi_core::board::{
    BoardSource, Epic, Issue, JiraBoard, StatusCategory, SyncState, TaskKind,
};

use super::{color_from_proto, color_to_proto};
use crate::convert::{millis_from_proto, millis_to_proto};
use crate::v1;

/// Core kind → wire enum value.
pub fn kind_to_proto(kind: Option<TaskKind>) -> i32 {
    let wire = match kind {
        None => v1::TaskKind::Unspecified,
        Some(TaskKind::Task) => v1::TaskKind::Task,
        Some(TaskKind::Story) => v1::TaskKind::Story,
        Some(TaskKind::Bug) => v1::TaskKind::Bug,
        Some(TaskKind::Epic) => v1::TaskKind::Epic,
        Some(TaskKind::Subtask) => v1::TaskKind::Subtask,
    };
    wire.into()
}

/// Wire enum value → core kind; unknown values are none.
pub fn kind_from_proto(value: i32) -> Option<TaskKind> {
    match v1::TaskKind::try_from(value).ok()? {
        v1::TaskKind::Unspecified => None,
        v1::TaskKind::Task => Some(TaskKind::Task),
        v1::TaskKind::Story => Some(TaskKind::Story),
        v1::TaskKind::Bug => Some(TaskKind::Bug),
        v1::TaskKind::Epic => Some(TaskKind::Epic),
        v1::TaskKind::Subtask => Some(TaskKind::Subtask),
    }
}

fn category_to_proto(c: StatusCategory) -> i32 {
    match c {
        StatusCategory::ToDo => v1::JiraStatusCategory::ToDo,
        StatusCategory::InProgress => v1::JiraStatusCategory::InProgress,
        StatusCategory::Done => v1::JiraStatusCategory::Done,
    }
    .into()
}

fn category_from_proto(value: i32) -> StatusCategory {
    match v1::JiraStatusCategory::try_from(value) {
        Ok(v1::JiraStatusCategory::InProgress) => StatusCategory::InProgress,
        Ok(v1::JiraStatusCategory::Done) => StatusCategory::Done,
        _ => StatusCategory::ToDo,
    }
}

/// Core source → wire (`None` for a board of Urushi's own).
pub fn source_to_proto(source: Option<&BoardSource>) -> Option<v1::JiraBoardSource> {
    let BoardSource::Jira(j) = source?;
    Some(v1::JiraBoardSource {
        site: j.site.clone(),
        board_id: j.board_id,
        name: j.name.clone(),
        project_key: j.project_key.clone(),
        scrum: j.scrum,
        synced_at_ms: j.sync.synced_at.map_or(0, millis_to_proto),
        error: j.sync.error.clone().unwrap_or_default(),
        dropped: j.sync.dropped,
        url: j.board_url(),
    })
}

/// Wire source → core. The rank field is the daemon's and isn't sent.
pub fn source_from_proto(source: Option<&v1::JiraBoardSource>) -> Option<BoardSource> {
    let j = source?;
    Some(BoardSource::Jira(JiraBoard {
        site: j.site.clone(),
        board_id: j.board_id,
        name: j.name.clone(),
        project_key: j.project_key.clone(),
        scrum: j.scrum,
        rank_field: None,
        sync: SyncState {
            synced_at: (j.synced_at_ms != 0).then(|| millis_from_proto(j.synced_at_ms)),
            error: (!j.error.is_empty()).then(|| j.error.clone()),
            dropped: j.dropped,
        },
    }))
}

/// Core epic → wire.
pub fn epic_to_proto(e: &Epic, site: &str) -> v1::Epic {
    v1::Epic {
        key: e.key.clone(),
        summary: e.summary.clone(),
        color: color_to_proto(Some(e.color)),
        url: format!("{site}/browse/{}", e.key),
    }
}

/// Wire epic → core; an unknown color reads as gray.
pub fn epic_from_proto(e: &v1::Epic) -> Epic {
    Epic {
        key: e.key.clone(),
        summary: e.summary.clone(),
        color: color_from_proto(e.color).unwrap_or(urushi_core::LabelColor::Gray),
    }
}

/// Core issue → wire, with its web address on `site`.
pub fn issue_to_proto(i: &Issue, site: Option<&str>) -> v1::JiraIssue {
    v1::JiraIssue {
        id: i.id.clone(),
        key: i.key.clone(),
        url: site.map_or_else(String::new, |s| format!("{s}/browse/{}", i.key)),
        type_name: i.type_name.clone(),
        status_id: i.status_id.clone(),
        status: i.status.clone(),
        category: category_to_proto(i.category),
        assignee: i.assignee.clone().unwrap_or_default(),
        priority: i.priority.clone().unwrap_or_default(),
        epic_key: i.epic.clone().unwrap_or_default(),
        notes_editable: i.notes_editable,
        parent_id: i.parent.clone().unwrap_or_default(),
        rank: i.rank.clone(),
        updated_ms: millis_to_proto(i.updated),
    }
}

/// Wire issue → core.
pub fn issue_from_proto(i: &v1::JiraIssue) -> Issue {
    let some = |s: &String| (!s.is_empty()).then(|| s.clone());
    Issue {
        id: i.id.clone(),
        key: i.key.clone(),
        type_name: i.type_name.clone(),
        status_id: i.status_id.clone(),
        status: i.status.clone(),
        category: category_from_proto(i.category),
        assignee: some(&i.assignee),
        priority: some(&i.priority),
        epic: some(&i.epic_key),
        parent: some(&i.parent_id),
        rank: i.rank.clone(),
        updated: millis_from_proto(i.updated_ms),
        notes_editable: i.notes_editable,
    }
}

#[cfg(test)]
mod tests {
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
}
