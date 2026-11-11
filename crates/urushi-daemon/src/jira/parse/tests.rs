//! Parsing answers shaped like Jira Cloud's (trimmed from real responses).

use serde_json::json;

use super::*;

fn story() -> Value {
    json!({
        "id": "10042", "key": "ACME-42",
        "fields": {
            "summary": "Checkout flow",
            "description": { "type": "doc", "version": 1, "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "Pay with cards" }] }
            ]},
            "issuetype": { "id": "10001", "name": "Story", "subtask": false, "hierarchyLevel": 0 },
            "status": { "id": "3", "name": "In Progress", "statusCategory": { "key": "indeterminate" } },
            "assignee": { "displayName": "Ada Lovelace", "accountId": "acc-ada" },
            "reporter": { "displayName": "Grace Hopper", "accountId": "acc-grace" },
            "priority": { "id": "2", "name": "High" },
            "duedate": "2026-12-01",
            "components": [{ "id": "100", "name": "API" }],
            "fixVersions": [{ "id": "200", "name": "1.0" }],
            "issuelinks": [{ "id": "1" }, { "id": "2" }],
            "customfield_10016": 3.0,
            "customfield_10020": [
                { "id": 5, "name": "Sprint 5", "state": "closed" },
                { "id": 6, "name": "Sprint 6", "state": "active" }
            ],
            "parent": { "id": "10007", "key": "ACME-7", "fields": {
                "summary": "Billing", "issuetype": { "hierarchyLevel": 1 } } },
            "labels": ["backend", "payments"],
            "updated": "2026-11-09T10:20:30.123+0000",
            "created": "2026-11-01T08:00:00.000+0000",
            "customfield_10019": "0|i0001b:"
        }
    })
}

fn rank() -> IssueFields {
    IssueFields {
        rank: Some("customfield_10019".into()),
        ..IssueFields::default()
    }
}

#[test]
fn a_story_reads_with_its_epic_status_and_rank() {
    let s = issue(&story(), &rank()).unwrap();
    assert_eq!(s.issue.key, "ACME-42");
    assert_eq!(s.kind, TaskKind::Story);
    assert_eq!(s.issue.status_id, "3");
    assert_eq!(s.issue.category, StatusCategory::InProgress);
    assert_eq!(s.issue.assignee.as_deref(), Some("Ada Lovelace"));
    assert_eq!(s.issue.priority.as_deref(), Some("High"));
    assert_eq!(s.issue.epic.as_deref(), Some("ACME-7"));
    assert_eq!(s.epic_summary.as_deref(), Some("Billing"));
    assert_eq!(s.issue.parent, None);
    assert_eq!(s.issue.rank, "0|i0001b:");
    assert_eq!(s.notes, "Pay with cards");
    assert_eq!(s.labels, ["backend", "payments"]);
    assert_eq!(s.issue.updated.unix_millis(), 1_794_219_630_123);
}

#[test]
fn a_subtask_names_its_parent_not_an_epic() {
    let mut v = story();
    v["fields"]["issuetype"] = json!({ "name": "Sub-task", "subtask": true });
    v["fields"]["parent"] =
        json!({ "id": "10041", "key": "ACME-40", "fields": { "summary": "Parent" } });
    let s = issue(&v, &IssueFields::default()).unwrap();
    assert_eq!(s.kind, TaskKind::Subtask);
    assert_eq!(s.issue.parent.as_deref(), Some("10041"));
    assert_eq!((s.issue.epic, s.epic_summary), (None, None));
    assert_eq!(s.issue.rank, "");
}

#[test]
fn missing_fields_read_as_empty_and_an_issue_needs_an_id_and_key() {
    let s = issue(
        &json!({ "id": "1", "key": "A-1", "fields": {} }),
        &IssueFields::default(),
    )
    .unwrap();
    assert_eq!((s.title.as_str(), s.notes.as_str()), ("", ""));
    assert_eq!(s.kind, TaskKind::Task);
    assert_eq!(s.issue.assignee, None);
    assert!(issue(&json!({ "key": "A-1" }), &IssueFields::default()).is_none());
}

#[test]
fn search_pages_end_without_a_token_or_on_the_last_page() {
    let page = json!({ "issues": [story(), { "bad": true }], "nextPageToken": "abc" });
    let (issues, next) = search(&page, &IssueFields::default());
    assert_eq!(issues.len(), 1);
    assert_eq!(next.as_deref(), Some("abc"));
    let last = json!({ "issues": [], "nextPageToken": "abc", "isLast": true });
    assert_eq!(search(&last, &IssueFields::default()).1, None);
}

#[test]
fn boards_and_their_configuration() {
    let info = board_info(&json!({
        "id": 7, "name": "ACME board", "type": "kanban",
        "location": { "projectKey": "ACME", "projectName": "Acme" }
    }))
    .unwrap();
    assert_eq!(
        (info.id, info.scrum, info.project_key.as_str()),
        (7, false, "ACME")
    );
    let config = config(
        info,
        &json!({
            "columnConfig": { "columns": [
                { "name": "To Do", "statuses": [{ "id": "1" }, { "id": "10000" }] },
                { "name": "Done", "statuses": [{ "id": "10001" }] }
            ]},
            "ranking": { "rankCustomFieldId": 10019 },
            "filter": { "id": "10003" },
            "subQuery": { "query": "fixVersion in unreleasedVersions() OR fixVersion is EMPTY" }
        }),
    )
    .unwrap();
    assert_eq!(config.columns[0].statuses, ["1", "10000"]);
    assert_eq!(config.rank_field.as_deref(), Some("customfield_10019"));
    assert_eq!(config.filter_id, 10003);
    assert!(config.sub_query.unwrap().starts_with("fixVersion"));
}

#[test]
fn transitions_and_issue_types() {
    let t = transitions(&json!({ "transitions": [
        { "id": "21", "name": "Start", "to": { "id": "3", "statusCategory": { "key": "indeterminate" } } },
        { "id": "31", "name": "Done", "to": { "id": "10001", "statusCategory": { "key": "done" } } }
    ]}));
    assert_eq!(t[1].to_status, "10001");
    assert_eq!(t[1].to_category, StatusCategory::Done);

    let types = json!({ "issueTypes": [
        { "id": "1", "name": "Task", "hierarchyLevel": 0 },
        { "id": "2", "name": "Story", "hierarchyLevel": 0 },
        { "id": "3", "name": "Subtask", "subtask": true, "hierarchyLevel": -1 },
        { "id": "4", "name": "Epic", "hierarchyLevel": 1 }
    ]});
    assert_eq!(issue_type(&types, TaskKind::Story).as_deref(), Some("2"));
    assert_eq!(
        issue_type(&types, TaskKind::Bug).as_deref(),
        Some("1"),
        "falls back to Task"
    );
    assert_eq!(issue_type(&types, TaskKind::Subtask).as_deref(), Some("3"));
}

#[test]
fn error_messages_combine_both_kinds() {
    let body = br#"{"errorMessages":["Nope"],"errors":{"labels":"no spaces"}}"#;
    assert_eq!(error_message(body), "Nope; labels: no spaces");
    assert_eq!(error_message(b"<html>"), "");
}

#[test]
fn people_estimates_sprints_and_versions_are_read_with_their_ids() {
    let fields = IssueFields {
        points: Some("customfield_10016".into()),
        sprint: Some("customfield_10020".into()),
        ..rank()
    };
    let i = issue(&story(), &fields).unwrap().issue;
    assert_eq!(i.type_id, "10001");
    assert_eq!(i.assignee_id.as_deref(), Some("acc-ada"));
    let reporter = i.reporter.unwrap();
    assert_eq!(
        (reporter.account_id.as_str(), reporter.name.as_str()),
        ("acc-grace", "Grace Hopper")
    );
    assert_eq!(i.priority_id.as_deref(), Some("2"));
    assert_eq!(i.due.as_deref(), Some("2026-12-01"));
    assert_eq!(i.points.as_deref(), Some("3"));
    assert_eq!(i.sprint.map(|s| s.name).as_deref(), Some("Sprint 6"));
    assert_eq!(i.components[0].name, "API");
    assert_eq!(i.fix_versions[0].id, "200");
    assert_eq!(i.links, 2);
    // Without the board's fields, no estimate or sprint.
    let plain = issue(&story(), &IssueFields::default()).unwrap().issue;
    assert_eq!((plain.points, plain.sprint), (None, None));
    let mut half = story();
    half["fields"]["customfield_10016"] = json!(0.5);
    assert_eq!(
        issue(&half, &fields).unwrap().issue.points.as_deref(),
        Some("0.5")
    );
}
