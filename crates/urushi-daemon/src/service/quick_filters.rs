//! A board's quick filters (ADR 0037): making, changing and removing them.
//!
//! A board of Urushi's own keeps its quick filters itself (`Board.quick_filters`). A
//! Jira board's quick filters are Jira's: they are written to Jira (under the board's
//! lock, like any Jira write), read back and kept in the board's mirror and its cached
//! configuration, so the next sync agrees. If Jira won't take a new one (the endpoint is
//! gone, or the account may not change the board), the board keeps it as JQL that Jira
//! still evaluates, and the answer says why. JQL is checked with a search first, so a
//! typo is refused rather than kept.

use urushi_core::board::{JiraQuickFilter, QuickFilterRef, quick_filter_name};
use urushi_core::{BoardError, BoardId, QuickFilterId, QuickRule};

use super::jira::board_query;
use super::{Service, ServiceError, ServiceResult, blocking, lock, new_uuid};
use crate::jira::{Account, JiraApi, JiraError, QuickFilterDraft};

/// Where a new quick filter went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickFilterSaved {
    /// The filter: in Jira, or kept by the board.
    pub filter: QuickFilterRef,
    /// Set when the board follows Jira but Jira wouldn't take it: why.
    pub kept_here: Option<String>,
}

/// What Jira did with a new quick filter.
enum Created {
    /// Made it; its id (if known) and the board's quick filters after (if read).
    InJira(Option<u64>, Option<Vec<JiraQuickFilter>>),
    /// Wouldn't: why.
    Refused(String),
}

impl Service {
    /// Adds a quick filter to `board`: kept here on a board of Urushi's own, made in
    /// Jira on a Jira board (or kept here, when Jira won't take it).
    pub async fn create_quick_filter(
        &self,
        board: BoardId,
        name: &str,
        rule: QuickRule,
    ) -> ServiceResult<QuickFilterSaved> {
        let jira = self.read(|s| {
            Ok(s.boards
                .iter()
                .find(|b| b.id == board)
                .map(|b| b.jira().cloned()))
        })?;
        let Some(jira) = jira.ok_or_else(|| ServiceError::NotFound(format!("board {board}")))?
        else {
            return self.keep_quick_filter(board, name, rule, None);
        };
        let draft = QuickFilterDraft {
            name: quick_filter_name(name)?,
            jql: jira_rule(rule)?,
        };
        let account = self.jira_account(board)?;
        let guard = self.jira.lock(board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let cached = self.jira.cached_config(board);
        let before: Vec<u64> = jira.quick_filters.iter().map(|q| q.id).collect();
        let sent = draft.clone();
        let created = blocking(move || {
            check_jql(&*api, &account, cached, jira.board_id, &sent.jql)?;
            match api.create_quick_filter(&account, jira.board_id, &sent) {
                Ok(id) => Ok(Created::InJira(
                    id,
                    api.quick_filters(&account, jira.board_id).ok(),
                )),
                Err(
                    e @ (JiraError::NotFound(_) | JiraError::Forbidden(_) | JiraError::Rejected(_)),
                ) => Ok(Created::Refused(e.to_string())),
                Err(e) => Err(e.into()),
            }
        })
        .await?;
        match created {
            Created::InJira(id, list) => {
                let id = id.or_else(|| {
                    list.as_deref()?
                        .iter()
                        .find(|q| !before.contains(&q.id) && q.name == draft.name)
                        .map(|q| q.id)
                });
                if let Some(list) = list {
                    self.store_jira_quick_filters(board, list)?;
                } else {
                    self.poke(board);
                }
                let id = id.ok_or_else(|| {
                    ServiceError::Unavailable(
                        "Jira made the quick filter but didn't say which it is; it shows after the next sync".into(),
                    )
                })?;
                Ok(QuickFilterSaved {
                    filter: QuickFilterRef::Jira(id),
                    kept_here: None,
                })
            }
            Created::Refused(why) => self.keep_quick_filter(
                board,
                &draft.name,
                QuickRule::Jql { jql: draft.jql },
                Some(why),
            ),
        }
    }

    /// Renames a quick filter and/or replaces its rule; `None` keeps either. A Jira
    /// board's own is changed in Jira.
    pub async fn update_quick_filter(
        &self,
        board: BoardId,
        filter: QuickFilterRef,
        name: Option<&str>,
        rule: Option<QuickRule>,
    ) -> ServiceResult<()> {
        let id = match filter {
            QuickFilterRef::Kept(id) => {
                return self
                    .mutate(|s| Ok(s.board_mut(board)?.update_quick_filter(id, name, rule)?));
            }
            QuickFilterRef::Jira(id) => id,
        };
        let jira = self.read(|s| s.jira_board(board))?;
        let current = jira_filter(&jira.quick_filters, id)?;
        let draft = QuickFilterDraft {
            name: name
                .map(quick_filter_name)
                .transpose()?
                .unwrap_or(current.name),
            jql: rule
                .map(jira_rule)
                .transpose()?
                .unwrap_or(current.jql.clone()),
        };
        let check = draft.jql != current.jql;
        self.write_jira_quick_filters(board, move |api, account, cached| {
            if check {
                check_jql(api, account, cached, jira.board_id, &draft.jql)?;
            }
            Ok(api.update_quick_filter(account, jira.board_id, id, &draft)?)
        })
        .await
    }

    /// Removes a quick filter; a Jira board's own is deleted in Jira.
    pub async fn delete_quick_filter(
        &self,
        board: BoardId,
        filter: QuickFilterRef,
    ) -> ServiceResult<()> {
        let id = match filter {
            QuickFilterRef::Kept(id) => {
                return self
                    .mutate(|s| Ok(s.board_mut(board)?.remove_quick_filter(id).map(drop)?));
            }
            QuickFilterRef::Jira(id) => id,
        };
        let jira = self.read(|s| s.jira_board(board))?;
        jira_filter(&jira.quick_filters, id)?;
        self.write_jira_quick_filters(board, move |api, account, _| {
            Ok(api.delete_quick_filter(account, jira.board_id, id)?)
        })
        .await
    }

    /// Keeps a quick filter on the board itself.
    fn keep_quick_filter(
        &self,
        board: BoardId,
        name: &str,
        rule: QuickRule,
        kept_here: Option<String>,
    ) -> ServiceResult<QuickFilterSaved> {
        let id = QuickFilterId::from_uuid(new_uuid());
        self.mutate(|s| Ok(s.board_mut(board)?.add_quick_filter(id, name, rule)?))?;
        Ok(QuickFilterSaved {
            filter: QuickFilterRef::Kept(id),
            kept_here,
        })
    }

    /// Makes a change to a Jira board's quick filters in Jira, under the board's lock,
    /// then reads them back. If Jira refuses, nothing changes here.
    async fn write_jira_quick_filters(
        &self,
        board: BoardId,
        write: impl FnOnce(
            &dyn JiraApi,
            &Account,
            Option<crate::jira::BoardConfig>,
        ) -> ServiceResult<()>
        + Send
        + 'static,
    ) -> ServiceResult<()> {
        let account = self.jira_account(board)?;
        let jira = self.read(|s| s.jira_board(board))?;
        let guard = self.jira.lock(board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let cached = self.jira.cached_config(board);
        let list = blocking(move || {
            write(&*api, &account, cached)?;
            Ok(api.quick_filters(&account, jira.board_id).ok())
        })
        .await?;
        if let Some(list) = list {
            self.store_jira_quick_filters(board, list)
        } else {
            self.poke(board);
            Ok(())
        }
    }

    /// Keeps a Jira board's quick filters as just read, in its mirror and in its cached
    /// configuration (which syncs copy them from).
    fn store_jira_quick_filters(
        &self,
        board: BoardId,
        list: Vec<JiraQuickFilter>,
    ) -> ServiceResult<()> {
        if let Some(config) = lock(&self.jira.boards)
            .get_mut(&board)
            .and_then(|s| s.config.as_mut())
        {
            config.quick_filters.clone_from(&list);
        }
        self.mutate_if_changed(|s| {
            let Some(j) = s.board_mut(board)?.jira_mut() else {
                return Ok(false);
            };
            let changed = j.quick_filters != list;
            j.quick_filters = list;
            Ok(changed)
        })?;
        Ok(())
    }
}

/// The JQL of a rule for a Jira board, checked.
fn jira_rule(rule: QuickRule) -> ServiceResult<String> {
    match rule.checked()? {
        QuickRule::Jql { jql } => Ok(jql),
        QuickRule::Fields { .. } => {
            Err(BoardError::WrongQuickRule("a Jira board's quick filters are JQL").into())
        }
    }
}

/// A Jira board's own quick filter, by id.
fn jira_filter(filters: &[JiraQuickFilter], id: u64) -> ServiceResult<JiraQuickFilter> {
    filters
        .iter()
        .find(|q| q.id == id)
        .cloned()
        .ok_or_else(|| ServiceError::NotFound(format!("quick filter {id}")))
}

/// Refuses JQL Jira can't read, by searching the board with it.
fn check_jql(
    api: &dyn JiraApi,
    account: &Account,
    cached: Option<crate::jira::BoardConfig>,
    board: u64,
    jql: &str,
) -> ServiceResult<()> {
    let config = match cached {
        Some(config) => config,
        None => api.board(account, board)?,
    };
    let mut query = board_query(&config);
    query.extra_jql = vec![jql.to_owned()];
    query.limit = 1;
    match api.issue_ids(account, &query) {
        Ok(_) => Ok(()),
        Err(JiraError::Rejected(why)) => Err(ServiceError::Invalid(format!(
            "Jira can't use that JQL: {why}"
        ))),
        Err(e) => Err(e.into()),
    }
}
