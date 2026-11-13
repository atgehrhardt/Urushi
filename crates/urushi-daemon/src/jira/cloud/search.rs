//! [`CloudJira`]'s searches for a board's issues: the board's JQL (its filter, sub-query,
//! open sprints, recent done issues, and any extra clauses such as quick filters),
//! paged by `nextPageToken` with guards against paging that doesn't end.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::{CloudJira, PAGE};
use crate::http::{Method, encode_query};
use crate::jira::api::{Fetched, IssueQuery, MAX_RECONCILE};
use crate::jira::{Account, DONE_DAYS, JiraResult, parse};

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
    clauses.extend(
        query
            .extra_jql
            .iter()
            .filter(|q| !q.trim().is_empty())
            .map(|q| format!("({q})")),
    );
    format!("{} ORDER BY Rank ASC", clauses.join(" AND "))
}

impl CloudJira {
    /// One page of a search for `fields`. A GET, or a POST when issues just written are
    /// to be reconciled: Jira takes `reconcileIssues` only in a body.
    fn search_page(
        &self,
        account: &Account,
        query: &IssueQuery,
        fields: &str,
        token: Option<&str>,
    ) -> JiraResult<Value> {
        let reconcile: Vec<u64> = query
            .reconcile
            .iter()
            .filter_map(|id| id.parse().ok())
            .take(MAX_RECONCILE)
            .collect();
        if reconcile.is_empty() {
            let next = token
                .map(|t| format!("&nextPageToken={}", encode_query(t)))
                .unwrap_or_default();
            let path = format!(
                "/rest/api/3/search/jql?jql={}&fields={}&maxResults={PAGE}{next}",
                encode_query(&jql(query)),
                encode_query(fields),
            );
            return self.get(account, &path);
        }
        let mut body = json!({
            "jql": jql(query),
            "fields": fields.split(',').collect::<Vec<_>>(),
            "maxResults": PAGE,
            "reconcileIssues": reconcile,
        });
        if let Some(t) = token {
            body["nextPageToken"] = json!(t);
        }
        self.call(account, Method::Post, "/rest/api/3/search/jql", Some(&body))
    }

    pub(super) fn search_issues(
        &self,
        account: &Account,
        query: &IssueQuery,
    ) -> JiraResult<Fetched> {
        let fields = Self::fields(&query.fields);
        let mut fetched = Fetched::default();
        let mut seen = HashSet::new();
        let mut token: Option<String> = None;
        // Guards against paging that doesn't end (tokens that repeat, or that lead back
        // to pages already read): a bounded number of pages, each adding something new.
        for _ in 0..=query.limit / PAGE + 1 {
            let page = self.search_page(account, query, &fields, token.as_deref())?;
            let (issues, next) = parse::search(&page, &query.fields);
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

    /// The board's issues the query matches (at most its limit) with only `fields`, as
    /// Jira sent them: for cheap looks (ids, update times).
    pub(super) fn search_fields(
        &self,
        account: &Account,
        query: &IssueQuery,
        fields: &str,
    ) -> JiraResult<Vec<Value>> {
        let mut found: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();
        let mut token: Option<String> = None;
        for _ in 0..=query.limit / PAGE + 1 {
            let page = self.search_page(account, query, fields, token.as_deref())?;
            let before = found.len();
            found.extend(
                page["issues"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|i| seen.insert(i["id"].to_string()))
                    .cloned(),
            );
            if found.len() >= query.limit {
                found.truncate(query.limit);
                break;
            }
            match parse::next_page(&page) {
                Some(next) if found.len() > before && token.as_ref() != Some(&next) => {
                    token = Some(next);
                }
                _ => break,
            }
        }
        Ok(found)
    }
}
