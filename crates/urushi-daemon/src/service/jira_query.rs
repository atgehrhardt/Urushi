//! Asking Jira which of a board's issues its quick filters let through (protocol
//! 1.13). Quick filters are JQL, which only Jira evaluates; clients filter by
//! everything else themselves, from the fields every issue carries.

use urushi_core::BoardId;

use super::jira::board_query;
use super::{Service, ServiceError, ServiceResult, blocking};

impl Service {
    /// The ids of the board's issues that every quick filter in `filters` lets through
    /// (at most the board's bound). Unknown filter ids are refused.
    pub async fn query_jira_filter(
        &self,
        board: BoardId,
        filters: &[u64],
    ) -> ServiceResult<Vec<String>> {
        let jira = self.read(|state| state.jira_board(board))?;
        let extra = filters
            .iter()
            .map(|id| {
                jira.quick_filters
                    .iter()
                    .find(|q| q.id == *id)
                    .map(|q| q.jql.clone())
                    .ok_or_else(|| ServiceError::NotFound(format!("quick filter {id}")))
            })
            .collect::<ServiceResult<Vec<_>>>()?;
        let account = self.jira_account()?;
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
