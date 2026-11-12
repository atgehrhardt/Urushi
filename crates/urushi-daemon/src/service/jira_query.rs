//! Asking Jira which of a board's issues its quick filters let through (protocol
//! 1.13). Quick filters are JQL, which only Jira evaluates; clients filter by
//! everything else themselves, from the fields every issue carries.

use urushi_core::{BoardId, QuickFilterId};

use super::jira::board_query;
use super::{Service, ServiceError, ServiceResult, blocking};

impl Service {
    /// The ids of the board's issues that every quick filter in `filters` (Jira's own,
    /// by id) and `kept` (the board's own JQL filters, ADR 0037) lets through (at most
    /// the board's bound). Unknown filters are refused.
    pub async fn query_jira_filter(
        &self,
        board: BoardId,
        filters: &[u64],
        kept: &[QuickFilterId],
    ) -> ServiceResult<Vec<String>> {
        let (jira, kept) = self.read(|state| {
            let jira = state.jira_board(board)?;
            let b = state.boards.iter().find(|b| b.id == board);
            let kept = kept
                .iter()
                .map(|id| {
                    b.and_then(|b| b.quick_filter(*id))
                        .and_then(|q| q.rule.jql())
                        .map(str::to_owned)
                        .ok_or_else(|| ServiceError::NotFound(format!("quick filter {id}")))
                })
                .collect::<ServiceResult<Vec<_>>>()?;
            Ok((jira, kept))
        })?;
        let mut extra = filters
            .iter()
            .map(|id| {
                jira.quick_filters
                    .iter()
                    .find(|q| q.id == *id)
                    .map(|q| q.jql.clone())
                    .ok_or_else(|| ServiceError::NotFound(format!("quick filter {id}")))
            })
            .collect::<ServiceResult<Vec<_>>>()?;
        extra.extend(kept);
        let account = self.jira_account(board)?;
        let cached = self.jira.cached_config(board);
        let api = self.jira.api.clone();
        blocking(move || {
            let config = match cached {
                Some(config) => config,
                None => api.board(&account, jira.board_id)?,
            };
            let mut query = board_query(&config);
            query.extra_jql = extra;
            Ok(api.issue_ids(&account, &query)?)
        })
        .await
    }
}
