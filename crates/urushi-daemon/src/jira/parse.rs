//! Jira's JSON answers → the daemon's types. Lenient: a missing or odd field reads as
//! empty, so an issue still shows when Jira adds, renames or hides a field; only an
//! issue without an id or key is skipped.

use serde_json::Value;
use urushi_core::board::{
    Issue, JiraColumn, JiraQuickFilter, Named, Person, StatusCategory, SyncedIssue,
};
use urushi_core::{TaskKind, Timestamp};

use super::adf;
use super::api::{BoardConfig, BoardInfo, IssueFields, Transition};

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn some_text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// An id Jira sends as a string or a number.
fn id(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

fn time(v: &Value) -> Timestamp {
    v.as_str()
        .and_then(crate::time::rfc3339_ms)
        .and_then(|ms| u64::try_from(ms).ok())
        .map_or_else(Timestamp::unknown, Timestamp::from_unix_millis)
}

/// A user field (`assignee`, `reporter`).
pub(super) fn person(v: &Value) -> Option<Person> {
    Some(Person {
        account_id: some_text(&v["accountId"])?,
        name: text(&v["displayName"]),
    })
}

/// Something with an id and a name (a component, a version, a sprint).
fn named(v: &Value) -> Option<Named> {
    let id = some_text(&v["id"]).or_else(|| v["id"].as_u64().map(|n| n.to_string()))?;
    Some(Named {
        id,
        name: text(&v["name"]),
    })
}

fn named_list(v: &Value) -> Vec<Named> {
    v.as_array()
        .map(|a| a.iter().filter_map(named).collect())
        .unwrap_or_default()
}

/// The sprint an issue is in: the active one, else a future one, else the last.
fn sprint(v: &Value) -> Option<Named> {
    let sprints = v.as_array()?;
    let state = |s: &&Value| s["state"].as_str().unwrap_or_default().to_owned();
    sprints
        .iter()
        .find(|s| state(s) == "active")
        .or_else(|| sprints.iter().find(|s| state(s) == "future"))
        .or_else(|| sprints.last())
        .and_then(named)
}

/// An estimate as Jira would show it: `3`, not `3.0`.
fn points(v: &Value) -> Option<String> {
    let n = v.as_f64()?;
    let text = format!("{n}");
    Some(
        text.strip_suffix(".0")
            .map_or_else(|| text.clone(), str::to_owned),
    )
}

/// The hierarchy level of an issue type: `1` epics, `-1` subtasks, `0` the rest.
fn level(issue_type: &Value) -> i32 {
    issue_type["hierarchyLevel"]
        .as_i64()
        .and_then(|l| i32::try_from(l).ok())
        .unwrap_or(if issue_type["subtask"].as_bool() == Some(true) {
            -1
        } else {
            0
        })
}

/// One issue of a search or a `GET issue`.
pub(super) fn issue(v: &Value, fields: &IssueFields) -> Option<SyncedIssue> {
    let f = &v["fields"];
    let type_name = text(&f["issuetype"]["name"]);
    let kind = TaskKind::from_jira(&type_name, level(&f["issuetype"]));
    let parent = &f["parent"];
    let (parent_id, epic, epic_summary) = match kind {
        TaskKind::Subtask => (some_text(&parent["id"]), None, None),
        TaskKind::Epic => (None, None, None),
        // A standard issue's parent is its epic.
        _ => (
            None,
            some_text(&parent["key"]),
            some_text(&parent["fields"]["summary"]),
        ),
    };
    let status = &f["status"];
    Some(SyncedIssue {
        issue: Issue {
            id: some_text(&v["id"])?,
            key: some_text(&v["key"])?,
            type_name,
            type_id: text(&f["issuetype"]["id"]),
            status_id: text(&status["id"]),
            status: text(&status["name"]),
            category: StatusCategory::from_jira(
                status["statusCategory"]["key"].as_str().unwrap_or_default(),
            ),
            assignee: some_text(&f["assignee"]["displayName"]),
            assignee_id: some_text(&f["assignee"]["accountId"]),
            reporter: person(&f["reporter"]),
            priority: some_text(&f["priority"]["name"]),
            priority_id: some_text(&f["priority"]["id"]),
            due: some_text(&f["duedate"]),
            points: fields.points.as_deref().and_then(|p| points(&f[p])),
            sprint: fields.sprint.as_deref().and_then(|s| sprint(&f[s])),
            components: named_list(&f["components"]),
            fix_versions: named_list(&f["fixVersions"]),
            links: f["issuelinks"]
                .as_array()
                .map_or(0, |l| u32::try_from(l.len()).unwrap_or(u32::MAX)),
            epic,
            parent: parent_id,
            rank: fields
                .rank
                .as_deref()
                .map(|r| text(&f[r]))
                .unwrap_or_default(),
            updated: time(&f["updated"]),
            notes_editable: true,
            // Read from development info for linked issues only (ADR 0036).
            pull_requests: Vec::new(),
        },
        title: text(&f["summary"]),
        notes: adf::to_markdown(&f["description"]),
        labels: f["labels"]
            .as_array()
            .map(|l| l.iter().filter_map(some_text).collect())
            .unwrap_or_default(),
        kind,
        epic_summary,
        created: time(&f["created"]),
    })
}

/// The issues of a search answer, and the token for the next page.
pub(super) fn search(v: &Value, fields: &IssueFields) -> (Vec<SyncedIssue>, Option<String>) {
    let issues = v["issues"]
        .as_array()
        .map(|a| a.iter().filter_map(|i| issue(i, fields)).collect())
        .unwrap_or_default();
    (issues, next_page(v))
}

/// An issue's id and when it last changed, from a search for `updated` only.
pub(super) fn stamp(v: &Value) -> Option<(String, Timestamp)> {
    Some((some_text(&v["id"])?, time(&v["fields"]["updated"])))
}

/// The token for a search answer's next page, if there is one.
pub(super) fn next_page(v: &Value) -> Option<String> {
    some_text(&v["nextPageToken"]).filter(|_| v["isLast"].as_bool() != Some(true))
}

/// A board's quick filters (`GET board/{id}/quickfilter`).
pub(super) fn quick_filters(v: &Value) -> Vec<JiraQuickFilter> {
    v["values"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|q| {
                    Some(JiraQuickFilter {
                        id: id(&q["id"])?,
                        name: text(&q["name"]),
                        jql: some_text(&q["jql"])?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The sprint field of a site's field list (`GET field`).
pub(super) fn sprint_field(v: &Value) -> Option<String> {
    v.as_array()?
        .iter()
        .find(|f| f["schema"]["custom"].as_str() == Some("com.pyxis.greenhopper.jira:gh-sprint"))
        .and_then(|f| some_text(&f["id"]))
}

/// A board of a board listing, or `GET board/{id}`.
pub(super) fn board_info(v: &Value) -> Option<BoardInfo> {
    let location = &v["location"];
    Some(BoardInfo {
        id: id(&v["id"])?,
        name: text(&v["name"]),
        scrum: v["type"].as_str() == Some("scrum"),
        project_key: text(&location["projectKey"]),
        project_name: some_text(&location["projectName"])
            .or_else(|| some_text(&location["displayName"]))
            .unwrap_or_default(),
    })
}

/// A board's configuration (`GET board/{id}/configuration`).
pub(super) fn config(info: BoardInfo, v: &Value) -> Option<BoardConfig> {
    let columns = v["columnConfig"]["columns"]
        .as_array()?
        .iter()
        .map(|c| JiraColumn {
            name: text(&c["name"]),
            statuses: c["statuses"]
                .as_array()
                .map(|s| s.iter().filter_map(|s| some_text(&s["id"])).collect())
                .unwrap_or_default(),
        })
        .collect();
    Some(BoardConfig {
        info,
        columns,
        rank_field: id(&v["ranking"]["rankCustomFieldId"]).map(|n| format!("customfield_{n}")),
        points_field: some_text(&v["estimation"]["field"]["fieldId"]),
        sprint_field: None,
        quick_filters: Vec::new(),
        filter_id: id(&v["filter"]["id"])?,
        sub_query: some_text(&v["subQuery"]["query"]),
    })
}

/// An issue's available transitions.
pub(super) fn transitions(v: &Value) -> Vec<Transition> {
    v["transitions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some(Transition {
                        id: some_text(&t["id"])?,
                        name: text(&t["name"]),
                        to_status: some_text(&t["to"]["id"])?,
                        to_category: StatusCategory::from_jira(
                            t["to"]["statusCategory"]["key"]
                                .as_str()
                                .unwrap_or_default(),
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The id of the project's issue type for `kind`: its own name ("Story", "Bug"), else
/// a type at the right level, else the first.
pub(super) fn issue_type(v: &Value, kind: TaskKind) -> Option<String> {
    // Newer answers list `issueTypes`; some list `values`.
    let types = v["issueTypes"]
        .as_array()
        .or_else(|| v["values"].as_array())?;
    let wanted_level = match kind {
        TaskKind::Subtask => -1,
        TaskKind::Epic => 1,
        _ => 0,
    };
    let at_level: Vec<&Value> = types.iter().filter(|t| level(t) == wanted_level).collect();
    at_level
        .iter()
        .find(|t| TaskKind::parse(t["name"].as_str().unwrap_or_default()) == Some(kind))
        .or_else(|| {
            at_level.iter().find(|t| {
                TaskKind::parse(t["name"].as_str().unwrap_or_default()) == Some(TaskKind::Task)
            })
        })
        .or_else(|| at_level.first())
        .and_then(|t| some_text(&t["id"]))
}

/// What Jira said went wrong: its `errorMessages` and field `errors`.
pub(super) fn error_message(body: &[u8]) -> String {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return String::new();
    };
    let mut messages: Vec<String> = v["errorMessages"]
        .as_array()
        .map(|m| m.iter().filter_map(some_text).collect())
        .unwrap_or_default();
    if let Some(errors) = v["errors"].as_object() {
        messages.extend(
            errors
                .iter()
                .filter_map(|(field, m)| Some(format!("{field}: {}", m.as_str()?))),
        );
    }
    messages.join("; ")
}

pub(super) mod details;
pub(super) mod dev;

#[cfg(test)]
mod tests;
