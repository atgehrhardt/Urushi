//! [`JiraApi`] for Jira Cloud's REST APIs (platform v3 and the agile API), over
//! [`HttpClient`].
//!
//! - Boards and their configuration come from the agile API (`/rest/agile/1.0/board`).
//! - Issues come from `/rest/api/3/search/jql` with the board's own filter, so
//!   descriptions arrive as ADF. Scrum boards are narrowed to open sprints, done issues
//!   to the last [`DONE_DAYS`] days (as Jira's kanban boards do), and incremental reads
//!   to issues updated in the last few minutes (a relative time, which avoids JQL's
//!   per-user time zones).
//! - Changes use the issue endpoints: transitions, edit (labels as add/remove
//!   operations, so labels Urushi can't show are never touched), create and rank.

use std::sync::Arc;

use serde_json::{Value, json};
use urushi_core::TaskKind;
use urushi_core::board::{Person, SyncedIssue};

use super::api::{
    BoardConfig, BoardInfo, Fetched, IssueEdit, IssueFields, IssueQuery, JiraApi, JiraError,
    JiraResult, Me, NewIssue, Rank, Transition,
};
use super::details::{Field, IssueDetails, IssueRef};
use super::{Account, adf, parse};
use crate::http::{HttpClient, Method, Request, base64, encode_query};

/// Done issues older than this many days aren't read.
pub const DONE_DAYS: u32 = 14;
/// Issues read per search page.
const PAGE: usize = 100;
/// The issue fields the board shows (with the board's own: [`IssueFields`]).
const FIELDS: &str = "summary,description,issuetype,status,assignee,reporter,priority,parent,\
                      labels,updated,created,duedate,components,fixVersions,issuelinks";
/// Most quick filters read for a board.
const QUICK_FILTERS: usize = 50;

/// Jira Cloud.
pub struct CloudJira {
    http: Arc<dyn HttpClient>,
}

impl CloudJira {
    /// Jira Cloud over `http`.
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http }
    }

    /// Calls `path` (with its query) on the account's site; the JSON answer, or `Null`
    /// for an empty one.
    fn call(
        &self,
        account: &Account,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> JiraResult<Value> {
        let mut request = Self::request(account, method, &format!("{}{path}", account.site));
        request.json = body.map(Value::to_string);
        let body = self.send(&request)?;
        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&body)
            .map_err(|_| JiraError::Unavailable("Jira sent an unreadable answer".into()))
    }

    /// A request to `url` signed in as the account.
    fn request(account: &Account, method: Method, url: &str) -> Request {
        let auth = base64(format!("{}:{}", account.email, account.token).as_bytes());
        Request::new(
            method,
            url.to_owned(),
            vec![
                ("Authorization".into(), format!("Basic {auth}")),
                ("Accept".into(), "application/json".into()),
            ],
        )
    }

    /// Sends a request; the body of a successful answer, or what went wrong.
    fn send(&self, request: &Request) -> JiraResult<Vec<u8>> {
        let answer = self
            .http
            .send(request)
            .map_err(|e| JiraError::Unavailable(e.0))?;
        if (200..=299).contains(&answer.status) {
            return Ok(answer.body);
        }
        let message = || {
            let m = parse::error_message(&answer.body);
            if m.is_empty() {
                format!("HTTP {}", answer.status)
            } else {
                m
            }
        };
        match answer.status {
            401 => Err(JiraError::Unauthorized),
            403 => Err(JiraError::Forbidden(message())),
            404 => Err(JiraError::NotFound(message())),
            429 => Err(JiraError::RateLimited {
                retry_after: answer.retry_after,
            }),
            400..=499 => Err(JiraError::Rejected(message())),
            _ => Err(JiraError::Unavailable(message())),
        }
    }

    fn get(&self, account: &Account, path: &str) -> JiraResult<Value> {
        self.call(account, Method::Get, path, None)
    }

    /// The issue type of `project` for `kind`, by id.
    fn issue_type(&self, account: &Account, project: &str, kind: TaskKind) -> JiraResult<String> {
        let types = self.get(
            account,
            &format!(
                "/rest/api/3/issue/createmeta/{}/issuetypes",
                segment(project)
            ),
        )?;
        parse::issue_type(&types, kind).ok_or_else(|| {
            JiraError::Rejected(format!(
                "project {project} has no issue type for a {}",
                kind.name()
            ))
        })
    }

    fn fields(fields: &IssueFields) -> String {
        std::iter::once(FIELDS)
            .chain(fields.ids())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The site's sprint field (from its field list), if it has one.
    fn sprint_field(&self, account: &Account) -> JiraResult<Option<String>> {
        Ok(parse::sprint_field(
            &self.get(account, "/rest/api/3/field")?,
        ))
    }
}

/// An issue key or id as a path segment.
fn segment(key: &str) -> String {
    encode_query(key)
}

impl JiraApi for CloudJira {
    fn myself(&self, account: &Account) -> JiraResult<Me> {
        let me = self.get(account, "/rest/api/3/myself")?;
        Ok(Me {
            display_name: me["displayName"]
                .as_str()
                .unwrap_or(&account.email)
                .to_owned(),
            account_id: me["accountId"].as_str().unwrap_or_default().to_owned(),
        })
    }

    fn boards(&self, account: &Account, query: &str) -> JiraResult<Vec<BoardInfo>> {
        let mut path = "/rest/agile/1.0/board?maxResults=50".to_owned();
        if !query.trim().is_empty() {
            path = format!("{path}&name={}", encode_query(query.trim()));
        }
        let page = self.get(account, &path)?;
        Ok(page["values"]
            .as_array()
            .map(|b| b.iter().filter_map(parse::board_info).collect())
            .unwrap_or_default())
    }

    fn board(&self, account: &Account, id: u64) -> JiraResult<BoardConfig> {
        let board = self.get(account, &format!("/rest/agile/1.0/board/{id}"))?;
        let info = parse::board_info(&board)
            .ok_or_else(|| JiraError::Unavailable("Jira sent an odd board".into()))?;
        let config = self.get(
            account,
            &format!("/rest/agile/1.0/board/{id}/configuration"),
        )?;
        let mut config = parse::config(info, &config)
            .ok_or_else(|| JiraError::Unavailable("Jira sent an odd board configuration".into()))?;
        // Extras: a board shows without them.
        config.quick_filters = self
            .get(
                account,
                &format!("/rest/agile/1.0/board/{id}/quickfilter?maxResults={QUICK_FILTERS}"),
            )
            .map(|v| parse::quick_filters(&v))
            .unwrap_or_default();
        config.sprint_field = self.sprint_field(account).ok().flatten();
        Ok(config)
    }

    fn issues(&self, account: &Account, query: &IssueQuery) -> JiraResult<Fetched> {
        self.search_issues(account, query)
    }

    fn issue_ids(&self, account: &Account, query: &IssueQuery) -> JiraResult<Vec<String>> {
        self.search_ids(account, query)
    }

    fn issue(&self, account: &Account, key: &str, fields: &IssueFields) -> JiraResult<SyncedIssue> {
        let path = format!(
            "/rest/api/3/issue/{}?fields={}",
            segment(key),
            encode_query(&Self::fields(fields))
        );
        parse::issue(&self.get(account, &path)?, fields)
            .ok_or_else(|| JiraError::Unavailable(format!("Jira sent an odd {key}")))
    }

    fn transitions(&self, account: &Account, key: &str) -> JiraResult<Vec<Transition>> {
        let path = format!("/rest/api/3/issue/{}/transitions", segment(key));
        Ok(parse::transitions(&self.get(account, &path)?))
    }

    fn transition(&self, account: &Account, key: &str, transition: &str) -> JiraResult<()> {
        let path = format!("/rest/api/3/issue/{}/transitions", segment(key));
        let body = json!({ "transition": { "id": transition } });
        self.call(account, Method::Post, &path, Some(&body))
            .map(drop)
    }

    fn edit(&self, account: &Account, key: &str, edit: &IssueEdit) -> JiraResult<()> {
        let mut fields = serde_json::Map::new();
        if let Some(summary) = &edit.summary {
            fields.insert("summary".into(), json!(summary));
        }
        if let Some(notes) = &edit.notes {
            fields.insert(
                "description".into(),
                adf::from_markdown(notes).unwrap_or(Value::Null),
            );
        }
        for (id, value) in &edit.fields {
            fields.insert(id.clone(), value.clone());
        }
        let labels: Vec<Value> = edit
            .labels_add
            .iter()
            .map(|l| json!({ "add": l }))
            .chain(edit.labels_remove.iter().map(|l| json!({ "remove": l })))
            .collect();
        let mut body = json!({ "fields": fields });
        if !labels.is_empty() {
            body["update"] = json!({ "labels": labels });
        }
        let path = format!("/rest/api/3/issue/{}", segment(key));
        self.call(account, Method::Put, &path, Some(&body))
            .map(drop)
    }

    fn create(&self, account: &Account, issue: &NewIssue) -> JiraResult<String> {
        let type_id = self.issue_type(account, &issue.project_key, issue.kind)?;
        let mut fields = json!({
            "project": { "key": issue.project_key },
            "issuetype": { "id": type_id },
            "summary": issue.summary,
        });
        if let Some(description) = adf::from_markdown(&issue.notes) {
            fields["description"] = description;
        }
        if !issue.labels.is_empty() {
            fields["labels"] = json!(issue.labels);
        }
        if let Some(parent) = &issue.parent_key {
            fields["parent"] = json!({ "key": parent });
        }
        for (id, value) in &issue.fields {
            fields[id] = value.clone();
        }
        let created = self.call(
            account,
            Method::Post,
            "/rest/api/3/issue",
            Some(&json!({ "fields": fields })),
        )?;
        created["key"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| JiraError::Unavailable("Jira didn't say which issue it made".into()))
    }

    fn rank(&self, account: &Account, key: &str, rank: &Rank) -> JiraResult<()> {
        let body = match rank {
            Rank::Before(other) => json!({ "issues": [key], "rankBeforeIssue": other }),
            Rank::After(other) => json!({ "issues": [key], "rankAfterIssue": other }),
        };
        self.call(
            account,
            Method::Put,
            "/rest/agile/1.0/issue/rank",
            Some(&body),
        )
        .map(drop)
    }

    fn details(&self, account: &Account, key: &str, board: u64) -> JiraResult<IssueDetails> {
        self.read_details(account, key, board)
    }

    fn create_fields(
        &self,
        account: &Account,
        project: &str,
        kind: TaskKind,
    ) -> JiraResult<Vec<Field>> {
        let type_id = self.issue_type(account, project, kind)?;
        self.read_create_fields(account, project, &type_id)
    }

    fn users(&self, account: &Account, query: &str, key: Option<&str>) -> JiraResult<Vec<Person>> {
        self.find_users(account, query, key)
    }

    fn search(
        &self,
        account: &Account,
        project: &str,
        query: &str,
        limit: usize,
    ) -> JiraResult<Vec<IssueRef>> {
        self.find_issues(account, project, query, limit)
    }

    fn add_comment(&self, account: &Account, key: &str, body: &str) -> JiraResult<()> {
        self.comment(account, key, None, Some(body))
    }

    fn update_comment(&self, account: &Account, key: &str, id: &str, body: &str) -> JiraResult<()> {
        self.comment(account, key, Some(id), Some(body))
    }

    fn delete_comment(&self, account: &Account, key: &str, id: &str) -> JiraResult<()> {
        self.comment(account, key, Some(id), None)
    }

    fn attach(&self, account: &Account, key: &str, name: &str, data: &[u8]) -> JiraResult<()> {
        self.upload(account, key, name, data)
    }

    fn delete_attachment(&self, account: &Account, id: &str) -> JiraResult<()> {
        let path = format!("/rest/api/3/attachment/{}", segment(id));
        self.call(account, Method::Delete, &path, None).map(drop)
    }

    fn download(&self, account: &Account, url: &str) -> JiraResult<Vec<u8>> {
        self.fetch(account, url)
    }

    fn link(&self, account: &Account, kind: &str, outward: &str, inward: &str) -> JiraResult<()> {
        let body = json!({
            "type": { "name": kind },
            "outwardIssue": { "key": outward },
            "inwardIssue": { "key": inward },
        });
        self.call(account, Method::Post, "/rest/api/3/issueLink", Some(&body))
            .map(drop)
    }

    fn unlink(&self, account: &Account, id: &str) -> JiraResult<()> {
        let path = format!("/rest/api/3/issueLink/{}", segment(id));
        self.call(account, Method::Delete, &path, None).map(drop)
    }

    fn watch(&self, account: &Account, key: &str, watch: bool) -> JiraResult<()> {
        let path = format!("/rest/api/3/issue/{}/watchers", segment(key));
        if watch {
            // The body is the account id as a JSON string.
            let me = json!(account.account_id);
            self.call(account, Method::Post, &path, Some(&me)).map(drop)
        } else {
            let path = format!("{path}?accountId={}", encode_query(&account.account_id));
            self.call(account, Method::Delete, &path, None).map(drop)
        }
    }

    fn log_work(
        &self,
        account: &Account,
        key: &str,
        seconds: u64,
        started: &str,
        comment: &str,
    ) -> JiraResult<()> {
        let mut body = json!({ "timeSpentSeconds": seconds, "started": started });
        if let Some(comment) = adf::from_markdown(comment) {
            body["comment"] = comment;
        }
        let path = format!("/rest/api/3/issue/{}/worklog", segment(key));
        self.call(account, Method::Post, &path, Some(&body))
            .map(drop)
    }
}

mod details;
mod search;

pub use search::jql;

#[cfg(test)]
mod tests;
