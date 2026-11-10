//! [`CloudJira`]'s searches for a board's issues: the board's JQL (its filter, sub-query,
//! open sprints, recent done issues, and any extra clauses such as quick filters),
//! paged by `nextPageToken` with guards against paging that doesn't end.

use std::collections::HashSet;

use serde_json::Value;

use super::{CloudJira, PAGE};
use crate::http::encode_query;
use crate::jira::api::{Fetched, IssueQuery};
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
    pub(super) fn search_issues(
        &self,
        account: &Account,
        query: &IssueQuery,
    ) -> JiraResult<Fetched> {
        let base = format!(
            "/rest/api/3/search/jql?jql={}&fields={}&maxResults={PAGE}",
            encode_query(&jql(query)),
            encode_query(&Self::fields(&query.fields)),
        );
        let mut fetched = Fetched::default();
        let mut seen = HashSet::new();
        let mut token: Option<String> = None;
        // Guards against paging that doesn't end (tokens that repeat, or that lead back
        // to pages already read): a bounded number of pages, each adding something new.
        for _ in 0..=query.limit / PAGE + 1 {
            let path = match &token {
                Some(t) => format!("{base}&nextPageToken={}", encode_query(t)),
                None => base.clone(),
            };
            let (issues, next) = parse::search(&self.get(account, &path)?, &query.fields);
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
        let base = format!(
            "/rest/api/3/search/jql?jql={}&fields={}&maxResults={PAGE}",
            encode_query(&jql(query)),
            encode_query(fields),
        );
        let mut found: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();
        let mut token: Option<String> = None;
        for _ in 0..=query.limit / PAGE + 1 {
            let path = match &token {
                Some(t) => format!("{base}&nextPageToken={}", encode_query(t)),
                None => base.clone(),
            };
            let page = self.get(account, &path)?;
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
