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
use urushi_core::board::SyncedIssue;

use super::api::{
    BoardConfig, BoardInfo, Fetched, IssueEdit, IssueQuery, JiraApi, JiraError, JiraResult,
    NewIssue, Rank, Transition,
};
use super::{Account, adf, parse};
use crate::http::{HttpClient, Method, Request, base64, encode_query};

/// Done issues older than this many days aren't read.
pub const DONE_DAYS: u32 = 14;
/// Issues read per search page.
const PAGE: usize = 100;
/// The issue fields the board shows.
const FIELDS: &str =
    "summary,description,issuetype,status,assignee,priority,parent,labels,updated,created";

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
        let auth = base64(format!("{}:{}", account.email, account.token).as_bytes());
        let request = Request {
            method,
            url: format!("{}{path}", account.site),
            headers: vec![
                ("Authorization".into(), format!("Basic {auth}")),
                ("Accept".into(), "application/json".into()),
            ],
            json: body.map(Value::to_string),
        };
        let answer = self
            .http
            .send(&request)
            .map_err(|e| JiraError::Unavailable(e.0))?;
        let message = || {
            let m = parse::error_message(&answer.body);
            if m.is_empty() {
                format!("HTTP {}", answer.status)
            } else {
                m
            }
        };
        match answer.status {
            200..=299 if answer.body.iter().all(u8::is_ascii_whitespace) => Ok(Value::Null),
            200..=299 => serde_json::from_slice(&answer.body)
                .map_err(|_| JiraError::Unavailable("Jira sent an unreadable answer".into())),
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

    fn fields(rank_field: Option<&str>) -> String {
        match rank_field {
            Some(rank) => format!("{FIELDS},{rank}"),
            None => FIELDS.into(),
        }
    }
}

/// The JQL for a board's issues.
pub fn jql(query: &IssueQuery) -> String {
    let mut clauses = vec![format!("filter = {}", query.filter_id)];
    if let Some(sub) = query.sub_query.as_deref().filter(|q| !q.trim().is_empty()) {
        clauses.push(format!("({sub})"));
    }
    if query.scrum {
        clauses.push("sprint in openSprints()".into());
    }
    clauses.push(format!(
        "(statusCategory != Done OR updated >= -{DONE_DAYS}d)"
    ));
    if let Some(minutes) = query.updated_within_minutes {
        clauses.push(format!("updated >= -{minutes}m"));
    }
    format!("{} ORDER BY Rank ASC", clauses.join(" AND "))
}

/// An issue key or id as a path segment.
fn segment(key: &str) -> String {
    encode_query(key)
}

impl JiraApi for CloudJira {
    fn myself(&self, account: &Account) -> JiraResult<String> {
        let me = self.get(account, "/rest/api/3/myself")?;
        Ok(me["displayName"]
            .as_str()
            .unwrap_or(&account.email)
            .to_owned())
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
        parse::config(info, &config)
            .ok_or_else(|| JiraError::Unavailable("Jira sent an odd board configuration".into()))
    }

    fn issues(&self, account: &Account, query: &IssueQuery) -> JiraResult<Fetched> {
        let base = format!(
            "/rest/api/3/search/jql?jql={}&fields={}&maxResults={PAGE}",
            encode_query(&jql(query)),
            encode_query(&Self::fields(query.rank_field.as_deref())),
        );
        let mut fetched = Fetched::default();
        let mut seen = std::collections::HashSet::new();
        let mut token: Option<String> = None;
        // Guards against paging that doesn't end (tokens that repeat, or that lead back
        // to pages already read): a bounded number of pages, each adding something new.
        for _ in 0..=query.limit / PAGE + 1 {
            let path = match &token {
                Some(t) => format!("{base}&nextPageToken={}", encode_query(t)),
                None => base.clone(),
            };
            let (issues, next) =
                parse::search(&self.get(account, &path)?, query.rank_field.as_deref());
            let before = fetched.issues.len();
            fetched.issues.extend(
                issues
                    .into_iter()
                    .filter(|i| seen.insert(i.issue.id.clone())),
            );
            if fetched.issues.len() >= query.limit {
                fetched.more = next.is_some() || fetched.issues.len() > query.limit;
                fetched.issues.truncate(query.limit);
                return Ok(fetched);
            }
            match next {
                Some(next) if fetched.issues.len() > before && token.as_ref() != Some(&next) => {
                    token = Some(next);
                }
                _ => return Ok(fetched),
            }
        }
        Ok(fetched)
    }

    fn issue(
        &self,
        account: &Account,
        key: &str,
        rank_field: Option<&str>,
    ) -> JiraResult<SyncedIssue> {
        let path = format!(
            "/rest/api/3/issue/{}?fields={}",
            segment(key),
            encode_query(&Self::fields(rank_field))
        );
        parse::issue(&self.get(account, &path)?, rank_field)
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
                adf::from_text(notes).unwrap_or(Value::Null),
            );
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
        let types = self.get(
            account,
            &format!(
                "/rest/api/3/issue/createmeta/{}/issuetypes",
                segment(&issue.project_key)
            ),
        )?;
        let type_id = parse::issue_type(&types, issue.kind).ok_or_else(|| {
            JiraError::Rejected(format!(
                "project {} has no issue type for a {}",
                issue.project_key,
                issue.kind.name()
            ))
        })?;
        let mut fields = json!({
            "project": { "key": issue.project_key },
            "issuetype": { "id": type_id },
            "summary": issue.summary,
        });
        if let Some(description) = adf::from_text(&issue.notes) {
            fields["description"] = description;
        }
        if !issue.labels.is_empty() {
            fields["labels"] = json!(issue.labels);
        }
        if let Some(parent) = &issue.parent_key {
            fields["parent"] = json!({ "key": parent });
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
}

#[cfg(test)]
mod tests;
