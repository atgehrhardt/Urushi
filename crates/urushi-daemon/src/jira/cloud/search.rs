//! [`CloudJira`]'s searches for a board's issues: the board's JQL (its filter, sub-query,
//! open sprints, recent done issues, and any extra clauses such as quick filters),
//! paged by `nextPageToken` with guards against paging that doesn't end.

use std::collections::HashSet;

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

    pub(super) fn search_ids(
        &self,
        account: &Account,
        query: &IssueQuery,
    ) -> JiraResult<Vec<String>> {
        let base = format!(
            "/rest/api/3/search/jql?jql={}&fields=id&maxResults={PAGE}",
            encode_query(&jql(query)),
        );
        let mut ids: Vec<String> = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..=query.limit / PAGE + 1 {
            let path = match &token {
                Some(t) => format!("{base}&nextPageToken={}", encode_query(t)),
                None => base.clone(),
            };
            let (page, next) = parse::search_ids(&self.get(account, &path)?);
            let before = ids.len();
            ids.extend(
                page.into_iter()
                    .filter(|id| !ids.contains(id))
                    .collect::<Vec<_>>(),
            );
            if ids.len() >= query.limit {
                ids.truncate(query.limit);
                break;
            }
            match next {
                Some(next) if ids.len() > before && token.as_ref() != Some(&next) => {
                    token = Some(next);
                }
                _ => break,
            }
        }
        Ok(ids)
    }
}
