//! The requests Jira Cloud gets, and how its answers are read.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use urushi_core::TaskKind;

use super::*;
use crate::http::{HttpError, Response};

/// Answers requests from a script and keeps them.
#[derive(Default)]
pub(super) struct Script {
    answers: Mutex<VecDeque<(u16, Value)>>,
    seen: Mutex<Vec<Request>>,
}

impl Script {
    pub(super) fn answering(answers: impl IntoIterator<Item = (u16, Value)>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().collect()),
            seen: Mutex::default(),
        })
    }

    pub(super) fn seen(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }
}

impl HttpClient for Script {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        self.seen.lock().unwrap().push(request.clone());
        let (status, body) = self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| HttpError("offline".into()))?;
        Ok(Response {
            status,
            retry_after: (status == 429).then(|| Duration::from_secs(90)),
            body: if body.is_null() {
                Vec::new()
            } else {
                body.to_string().into_bytes()
            },
        })
    }
}

pub(super) fn account() -> Account {
    Account {
        site: "https://acme.atlassian.net".into(),
        email: "a@b.c".into(),
        token: "t0k3n".into(),
        display_name: String::new(),
        account_id: String::new(),
    }
}

fn query() -> IssueQuery {
    IssueQuery {
        filter_id: 10003,
        sub_query: Some("fixVersion is EMPTY".into()),
        scrum: true,
        fields: IssueFields {
            rank: Some("customfield_10019".into()),
            points: None,
            sprint: None,
        },
        updated_within_minutes: Some(3),
        extra_jql: Vec::new(),
        limit: 3,
        reconcile: Vec::new(),
    }
}

fn found(key: &str) -> Value {
    json!({ "id": key.replace("A-", "1"), "key": key, "fields": { "summary": key } })
}

#[test]
fn requests_sign_in_with_basic_auth_on_the_accounts_site() {
    let http = Script::answering([(200, json!({ "displayName": "Ada", "accountId": "acc-1" }))]);
    let jira = CloudJira::new(http.clone());
    assert_eq!(
        jira.myself(&account()).unwrap(),
        Me {
            display_name: "Ada".into(),
            account_id: "acc-1".into()
        }
    );
    let r = &http.seen()[0];
    assert_eq!(r.url, "https://acme.atlassian.net/rest/api/3/myself");
    assert!(
        r.headers
            .contains(&("Authorization".into(), "Basic YUBiLmM6dDBrM24=".into()))
    );
}

#[test]
fn statuses_become_errors() {
    let cases = [
        (401, JiraError::Unauthorized),
        (403, JiraError::Forbidden("Nope".into())),
        (404, JiraError::NotFound("Nope".into())),
        (400, JiraError::Rejected("Nope".into())),
        (
            429,
            JiraError::RateLimited {
                retry_after: Some(Duration::from_secs(90)),
            },
        ),
        (503, JiraError::Unavailable("Nope".into())),
    ];
    for (status, expected) in cases {
        let http = Script::answering([(status, json!({ "errorMessages": ["Nope"] }))]);
        assert_eq!(CloudJira::new(http).myself(&account()), Err(expected));
    }
    let offline = CloudJira::new(Script::answering([]));
    assert!(matches!(
        offline.myself(&account()),
        Err(JiraError::Unavailable(_))
    ));
}

#[test]
fn board_jql_narrows_the_filter() {
    assert_eq!(
        jql(&query()),
        "filter = 10003 AND (fixVersion is EMPTY) AND sprint in openSprints() AND \
         (statusCategory != Done OR updated >= -14d) AND updated >= -3m ORDER BY Rank ASC"
    );
    let all = IssueQuery {
        sub_query: None,
        scrum: false,
        updated_within_minutes: None,
        ..query()
    };
    assert_eq!(
        jql(&all),
        "filter = 10003 AND (statusCategory != Done OR updated >= -14d) ORDER BY Rank ASC"
    );
    let quick = IssueQuery {
        extra_jql: vec!["assignee = currentUser()".into(), " ".into()],
        ..all
    };
    assert_eq!(
        jql(&quick),
        "filter = 10003 AND (statusCategory != Done OR updated >= -14d) AND \
         (assignee = currentUser()) ORDER BY Rank ASC"
    );
}

#[test]
fn quick_filters_are_evaluated_for_ids_only() {
    let http = Script::answering([
        (
            200,
            json!({ "issues": [{ "id": "11" }, { "id": "12" }], "nextPageToken": "p2" }),
        ),
        (200, json!({ "issues": [{ "id": "13" }], "isLast": true })),
    ]);
    let jira = CloudJira::new(http.clone());
    let q = IssueQuery {
        extra_jql: vec!["assignee = currentUser()".into()],
        limit: 10,
        ..query()
    };
    assert_eq!(jira.issue_ids(&account(), &q).unwrap(), ["11", "12", "13"]);
    let seen = http.seen();
    assert!(seen[0].url.contains("&fields=id&"), "{}", seen[0].url);
    assert!(
        seen[1].url.ends_with("&nextPageToken=p2"),
        "{}",
        seen[1].url
    );
}

#[test]
fn a_board_reads_its_quick_filters_estimate_and_sprint_fields() {
    let http = Script::answering([
        (200, json!({ "id": 7, "name": "ACME", "type": "scrum" })),
        (
            200,
            json!({
                "filter": { "id": "10003" },
                "columnConfig": { "columns": [{ "name": "To Do", "statuses": [{ "id": "1" }] }] },
                "estimation": { "type": "field", "field": { "fieldId": "customfield_10016" } }
            }),
        ),
        (
            200,
            json!({ "values": [{ "id": 4, "name": "Only my issues", "jql": "assignee = currentUser()" }] }),
        ),
        (
            200,
            json!([
                { "id": "summary", "schema": { "type": "string" } },
                { "id": "customfield_10020", "schema": { "custom": "com.pyxis.greenhopper.jira:gh-sprint" } }
            ]),
        ),
    ]);
    let config = CloudJira::new(http.clone()).board(&account(), 7).unwrap();
    assert_eq!(config.points_field.as_deref(), Some("customfield_10016"));
    assert_eq!(config.sprint_field.as_deref(), Some("customfield_10020"));
    assert_eq!(config.quick_filters.len(), 1);
    assert_eq!(config.quick_filters[0].jql, "assignee = currentUser()");
    let seen = http.seen();
    assert!(
        seen[2]
            .url
            .ends_with("/rest/agile/1.0/board/7/quickfilter?maxResults=50")
    );
    assert!(seen[3].url.ends_with("/rest/api/3/field"));
    // Boards whose extras can't be read still show.
    let bare = Script::answering([
        (200, json!({ "id": 7, "name": "ACME" })),
        (
            200,
            json!({ "filter": { "id": 1 }, "columnConfig": { "columns": [] } }),
        ),
        (403, json!({})),
        (403, json!({})),
    ]);
    let config = CloudJira::new(bare).board(&account(), 7).unwrap();
    assert!(config.quick_filters.is_empty() && config.sprint_field.is_none());
}

#[test]
fn searches_follow_pages_up_to_the_limit() {
    let http = Script::answering([
        (
            200,
            json!({ "issues": [found("A-1"), found("A-2")], "nextPageToken": "p2" }),
        ),
        (
            200,
            json!({ "issues": [found("A-3"), found("A-4")], "nextPageToken": "p3" }),
        ),
    ]);
    let fetched = CloudJira::new(http.clone())
        .issues(&account(), &query())
        .unwrap();
    let keys: Vec<_> = fetched
        .issues
        .iter()
        .map(|i| i.issue.key.as_str())
        .collect();
    assert_eq!(keys, ["A-1", "A-2", "A-3"]);
    assert!(fetched.more);
    let seen = http.seen();
    assert!(seen[0].url.contains("fields=summary%2C"));
    assert!(seen[0].url.contains("customfield_10019"));
    assert!(seen[1].url.ends_with("&nextPageToken=p2"));
}

#[test]
fn edits_send_fields_and_label_operations() {
    let http = Script::answering([(204, Value::Null)]);
    CloudJira::new(http.clone())
        .edit(
            &account(),
            "A-1",
            &IssueEdit {
                summary: Some("New".into()),
                notes: Some("line".into()),
                labels_add: vec!["ui".into()],
                labels_remove: vec!["old".into()],
                fields: vec![("priority".into(), json!({ "id": "2" }))],
            },
        )
        .unwrap();
    let r = &http.seen()[0];
    assert_eq!(r.method, Method::Put);
    let body: Value = serde_json::from_str(r.json.as_deref().unwrap()).unwrap();
    assert_eq!(body["fields"]["summary"], "New");
    assert_eq!(body["fields"]["description"]["type"], "doc");
    assert_eq!(body["fields"]["priority"]["id"], "2");
    assert_eq!(
        body["update"]["labels"],
        json!([{ "add": "ui" }, { "remove": "old" }])
    );
}

#[test]
fn creating_picks_the_projects_type_for_the_kind() {
    let http = Script::answering([
        (
            200,
            json!({ "issueTypes": [
                { "id": "1", "name": "Task", "hierarchyLevel": 0 },
                { "id": "5", "name": "Sub-task", "subtask": true, "hierarchyLevel": -1 }
            ]}),
        ),
        (201, json!({ "id": "10050", "key": "ACME-50" })),
    ]);
    let key = CloudJira::new(http.clone())
        .create(
            &account(),
            &NewIssue {
                project_key: "ACME".into(),
                kind: TaskKind::Subtask,
                parent_key: Some("ACME-1".into()),
                summary: "Step".into(),
                notes: String::new(),
                labels: Vec::new(),
                fields: vec![("customfield_1".into(), json!({ "id": "t1" }))],
            },
        )
        .unwrap();
    assert_eq!(key, "ACME-50");
    let seen = http.seen();
    assert!(
        seen[0]
            .url
            .ends_with("/rest/api/3/issue/createmeta/ACME/issuetypes")
    );
    let body: Value = serde_json::from_str(seen[1].json.as_deref().unwrap()).unwrap();
    assert_eq!(body["fields"]["issuetype"]["id"], "5");
    assert_eq!(body["fields"]["parent"]["key"], "ACME-1");
    assert_eq!(body["fields"]["customfield_1"]["id"], "t1");
    assert!(body["fields"].get("description").is_none());
}

#[test]
fn transitions_and_ranks() {
    let http = Script::answering([(204, Value::Null), (204, Value::Null)]);
    let jira = CloudJira::new(http.clone());
    jira.transition(&account(), "A-1", "31").unwrap();
    jira.rank(&account(), "A-1", &Rank::Before("A-2".into()))
        .unwrap();
    let seen = http.seen();
    assert!(seen[0].url.ends_with("/rest/api/3/issue/A-1/transitions"));
    assert_eq!(
        seen[0].json.as_deref(),
        Some(r#"{"transition":{"id":"31"}}"#)
    );
    assert_eq!(
        serde_json::from_str::<Value>(seen[1].json.as_deref().unwrap()).unwrap(),
        json!({ "issues": ["A-1"], "rankBeforeIssue": "A-2" })
    );
}

#[test]
fn paging_that_repeats_itself_ends() {
    // A token that leads back to the same page, as some Jira sites answer.
    let page = || {
        (
            200,
            json!({ "issues": [found("A-1")], "nextPageToken": "same" }),
        )
    };
    let http = Script::answering([page(), page(), page()]);
    let fetched = CloudJira::new(http.clone())
        .issues(
            &account(),
            &IssueQuery {
                limit: 500,
                ..query()
            },
        )
        .unwrap();
    assert_eq!(fetched.issues.len(), 1);
    assert!(http.seen().len() <= 2, "{}", http.seen().len());
}

#[test]
fn opening_an_issue_reads_its_fields_values_and_conversation() {
    let http = Script::answering([
        (
            200,
            json!({ "fields": {
                "summary": { "name": "Summary", "schema": { "type": "string", "system": "summary" } },
                "priority": { "name": "Priority", "required": false,
                    "schema": { "type": "priority", "system": "priority" },
                    "allowedValues": [{ "id": "1", "name": "Highest" }, { "id": "2", "name": "High" }] },
                "customfield_10020": { "name": "Sprint",
                    "schema": { "type": "array", "items": "json", "custom": "com.pyxis.greenhopper.jira:gh-sprint" } }
            }}),
        ),
        (
            200,
            json!({ "fields": {
                "priority": { "id": "2", "name": "High" },
                "customfield_10020": [{ "id": 6, "name": "Sprint 6", "state": "active" }],
                "attachment": [{ "id": "900", "filename": "a.png", "mimeType": "image/png", "size": 5,
                    "content": "https://acme.atlassian.net/rest/api/3/attachment/content/900" }],
                "issuelinks": [{ "id": "77", "type": { "name": "Blocks", "inward": "is blocked by", "outward": "blocks" },
                    "outwardIssue": { "id": "10002", "key": "A-2", "fields": { "summary": "Other", "status": { "name": "To Do" } } } }],
                "timetracking": { "timeSpent": "1h" }
            }}),
        ),
        (
            200,
            json!({ "values": [{ "id": 6, "name": "Sprint 6" }, { "id": 7, "name": "Sprint 7" }] }),
        ),
        (
            200,
            json!({ "comments": [{ "id": "5", "author": { "accountId": "acc-1", "displayName": "Ada" },
                "body": { "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "hi" }] }] },
                "created": "2026-11-09T10:20:30.123+0000" }] }),
        ),
        (
            200,
            json!({ "isWatching": true, "watchers": [{ "accountId": "acc-1", "displayName": "Ada" }] }),
        ),
        (
            200,
            json!({ "worklogs": [{ "id": "8", "timeSpentSeconds": 3600, "author": { "accountId": "acc-1" } }] }),
        ),
        (200, json!({ "transitions": [] })),
        (
            200,
            json!({ "issueLinkTypes": [{ "name": "Blocks", "inward": "is blocked by", "outward": "blocks" }] }),
        ),
    ]);
    let d = CloudJira::new(http.clone())
        .details(&account(), "A-1", 7)
        .unwrap();
    let names: Vec<&str> = d.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["Priority", "Sprint"],
        "the summary has its own place"
    );
    assert_eq!(d.fields[0].value.options[0].name, "High");
    assert_eq!(d.fields[0].allowed.len(), 2);
    assert_eq!(d.fields[1].allowed.len(), 2, "the board's sprints");
    assert_eq!(d.fields[1].value.options[0].id, "6");
    assert_eq!(d.comments[0].body, "hi");
    assert_eq!(d.attachments[0].name, "a.png");
    assert_eq!(
        (d.links[0].relation.as_str(), d.links[0].issue.key.as_str()),
        ("blocks", "A-2")
    );
    assert!(d.watching);
    assert_eq!(d.worklogs[0].seconds, 3600);
    assert_eq!(d.time.spent, "1h");
    assert_eq!(d.link_types[0].name, "Blocks");
    let seen = http.seen();
    assert!(
        seen[1]
            .url
            .contains("fields=priority%2Ccustomfield_10020%2Cattachment"),
        "{}",
        seen[1].url
    );
    assert!(
        seen[2]
            .url
            .ends_with("/rest/agile/1.0/board/7/sprint?state=active,future")
    );
}

#[test]
fn comments_attachments_links_and_watching_use_their_endpoints() {
    let http = Script::answering((0..7).map(|_| (204, Value::Null)));
    let jira = CloudJira::new(http.clone());
    let mut me = account();
    me.account_id = "acc-1".into();
    jira.add_comment(&me, "A-1", "**done**").unwrap();
    jira.delete_comment(&me, "A-1", "5").unwrap();
    jira.attach(&me, "A-1", "notes.txt", b"hello").unwrap();
    jira.link(&me, "Blocks", "A-1", "A-2").unwrap();
    jira.watch(&me, "A-1", true).unwrap();
    jira.watch(&me, "A-1", false).unwrap();
    jira.log_work(&me, "A-1", 1800, "2026-11-09T10:00:00.000+0000", "")
        .unwrap();
    let seen = http.seen();
    let body =
        |i: usize| -> Value { serde_json::from_str(seen[i].json.as_deref().unwrap()).unwrap() };
    assert!(seen[0].url.ends_with("/issue/A-1/comment"));
    assert_eq!(
        body(0)["body"]["content"][0]["content"][0]["marks"][0]["type"],
        "strong"
    );
    assert_eq!(
        (seen[1].method, seen[1].url.ends_with("/comment/5")),
        (Method::Delete, true)
    );
    assert!(seen[2].url.ends_with("/issue/A-1/attachments"));
    assert!(
        seen[2]
            .headers
            .contains(&("X-Atlassian-Token".into(), "no-check".into()))
    );
    assert_eq!(seen[2].upload.as_ref().unwrap().name, "notes.txt");
    assert_eq!(body(3)["outwardIssue"]["key"], "A-1");
    assert_eq!(body(4), json!("acc-1"));
    assert!(seen[5].url.ends_with("/watchers?accountId=acc-1"));
    assert_eq!(body(6)["timeSpentSeconds"], 1800);
    // Files are fetched from the site only.
    assert!(jira.download(&me, "https://evil.example/x").is_err());
}

#[test]
fn deleting_an_issue_takes_its_subtasks_and_reports_refusals() {
    let http = Script::answering([
        (204, Value::Null),
        (
            403,
            json!({ "errorMessages": ["You do not have permission to delete issues in this project."] }),
        ),
    ]);
    let jira = CloudJira::new(http.clone());
    jira.delete_issue(&account(), "A-1").unwrap();
    let seen = http.seen();
    assert_eq!(seen[0].method, Method::Delete);
    assert_eq!(
        seen[0].url,
        "https://acme.atlassian.net/rest/api/3/issue/A-1?deleteSubtasks=true"
    );
    assert_eq!(seen[0].json, None);
    assert!(matches!(
        jira.delete_issue(&account(), "A-2"),
        Err(JiraError::Forbidden(m)) if m.contains("permission")
    ));
}

#[test]
fn issues_just_written_are_searched_by_post_to_be_reconciled() {
    let http = Script::answering([(200, json!({ "issues": [{ "id": "11" }], "isLast": true }))]);
    let jira = CloudJira::new(http.clone());
    let q = IssueQuery {
        reconcile: vec!["10001".into(), "not-an-id".into()],
        limit: 10,
        ..query()
    };
    assert_eq!(jira.issue_ids(&account(), &q).unwrap(), ["11"]);
    let r = &http.seen()[0];
    assert_eq!(r.method, Method::Post);
    assert_eq!(r.url, "https://acme.atlassian.net/rest/api/3/search/jql");
    let body: Value = serde_json::from_str(r.json.as_deref().unwrap()).unwrap();
    assert_eq!(body["reconcileIssues"], json!([10001]));
    assert_eq!(body["fields"], json!(["id"]));
    assert_eq!(body["jql"], json!(jql(&q)));
}
