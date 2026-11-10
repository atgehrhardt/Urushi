//! The requests Jira Cloud gets, and how its answers are read.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use urushi_core::TaskKind;

use super::*;
use crate::http::{HttpError, Response};

/// Answers requests from a script and keeps them.
#[derive(Default)]
struct Script {
    answers: Mutex<VecDeque<(u16, Value)>>,
    seen: Mutex<Vec<Request>>,
}

impl Script {
    fn answering(answers: impl IntoIterator<Item = (u16, Value)>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().collect()),
            seen: Mutex::default(),
        })
    }

    fn seen(&self) -> Vec<Request> {
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

fn account() -> Account {
    Account {
        site: "https://acme.atlassian.net".into(),
        email: "a@b.c".into(),
        token: "t0k3n".into(),
        display_name: String::new(),
    }
}

fn query() -> IssueQuery {
    IssueQuery {
        filter_id: 10003,
        sub_query: Some("fixVersion is EMPTY".into()),
        scrum: true,
        rank_field: Some("customfield_10019".into()),
        updated_within_minutes: Some(3),
        limit: 3,
    }
}

fn found(key: &str) -> Value {
    json!({ "id": key.replace("A-", "1"), "key": key, "fields": { "summary": key } })
}

#[test]
fn requests_sign_in_with_basic_auth_on_the_accounts_site() {
    let http = Script::answering([(200, json!({ "displayName": "Ada" }))]);
    let jira = CloudJira::new(http.clone());
    assert_eq!(jira.myself(&account()).unwrap(), "Ada");
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
            },
        )
        .unwrap();
    let r = &http.seen()[0];
    assert_eq!(r.method, Method::Put);
    let body: Value = serde_json::from_str(r.json.as_deref().unwrap()).unwrap();
    assert_eq!(body["fields"]["summary"], "New");
    assert_eq!(body["fields"]["description"]["type"], "doc");
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
