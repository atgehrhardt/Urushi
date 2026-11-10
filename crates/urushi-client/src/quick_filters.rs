//! Typed wrappers for a board's quick filters (protocol 1.21, ADR 0033): making,
//! changing and removing them. On a Jira board the daemon writes them to Jira, or keeps
//! a new one itself when Jira won't take it.

use urushi_proto::v1::{self, request::Body, response::Outcome};

use crate::requests::unexpected;
use crate::{Client, ClientError};

/// First protocol minor version with quick filters that can be made, changed and
/// removed (and kept by boards of Urushi's own).
pub const QUICK_FILTERS_SINCE_MINOR: u32 = 21;

impl Client {
    /// Adds a quick filter to a board: JQL on a Jira board, the other criteria
    /// elsewhere. Says where it went.
    pub async fn create_quick_filter(
        &self,
        board_id: &str,
        name: &str,
        rule: v1::QuickFilterRule,
    ) -> Result<v1::QuickFilterSaved, ClientError> {
        let body = Body::CreateQuickFilter(v1::CreateQuickFilterRequest {
            board_id: board_id.into(),
            name: name.into(),
            rule: Some(rule),
        });
        match self.request(body).await? {
            Outcome::QuickFilterSaved(saved) => Ok(saved),
            _ => Err(unexpected("CreateQuickFilter")),
        }
    }

    /// Renames a quick filter (an empty name keeps it) and/or replaces its rule.
    pub async fn update_quick_filter(
        &self,
        board_id: &str,
        filter: v1::QuickFilterRef,
        name: &str,
        rule: Option<v1::QuickFilterRule>,
    ) -> Result<(), ClientError> {
        let body = Body::UpdateQuickFilter(v1::UpdateQuickFilterRequest {
            board_id: board_id.into(),
            filter: Some(filter),
            name: name.into(),
            rule,
        });
        self.ok(body, "UpdateQuickFilter").await
    }

    /// Removes a quick filter; a Jira board's own is deleted in Jira.
    pub async fn delete_quick_filter(
        &self,
        board_id: &str,
        filter: v1::QuickFilterRef,
    ) -> Result<(), ClientError> {
        let body = Body::DeleteQuickFilter(v1::DeleteQuickFilterRequest {
            board_id: board_id.into(),
            filter: Some(filter),
        });
        self.ok(body, "DeleteQuickFilter").await
    }

    /// The ids (`JiraIssue.id`) of a Jira board's issues that all of these quick
    /// filters let through: Jira's own by id, and the board's own JQL filters
    /// (`Board.quick_filters`) by id.
    pub async fn query_quick_filters(
        &self,
        board_id: &str,
        jira_ids: &[u64],
        kept_ids: &[String],
    ) -> Result<Vec<String>, ClientError> {
        let body = Body::QueryJiraFilter(v1::QueryJiraFilterRequest {
            board_id: board_id.into(),
            quick_filter_ids: jira_ids.to_vec(),
            kept_quick_filter_ids: kept_ids.to_vec(),
        });
        match self.request(body).await? {
            Outcome::JiraIssueIds(ids) => Ok(ids.ids),
            _ => Err(unexpected("QueryJiraFilter")),
        }
    }
}

/// A Jira board's own quick filter.
pub fn jira_filter(id: u64) -> v1::QuickFilterRef {
    v1::QuickFilterRef {
        filter: Some(v1::quick_filter_ref::Filter::JiraId(id)),
    }
}

/// A quick filter a board keeps.
pub fn kept_filter(id: &str) -> v1::QuickFilterRef {
    v1::QuickFilterRef {
        filter: Some(v1::quick_filter_ref::Filter::Id(id.into())),
    }
}
