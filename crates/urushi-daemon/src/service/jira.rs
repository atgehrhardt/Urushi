//! Jira boards in the service (ADR 0020): the sign-in, importing boards, and keeping
//! them in sync.
//!
//! **Syncing** reads Jira outside the state lock and merges what it read in one
//! `mutate` ([`urushi_core::Board::merge_issues`]), which only bumps the revision when
//! the board changed, so idle polls wake no client. A board is read:
//! - every [`ACTIVE_INTERVAL`] while a client watches the workspace, and every
//!   [`IDLE_INTERVAL`] otherwise: issues updated since the last read;
//! - completely on import, on `SyncBoard`, and every [`FULL_INTERVAL`]: the board's
//!   configuration (columns) and every issue, so deleted and moved-away issues go.
//!
//! Failures are recorded on the board for clients to show, and back off: as long as a
//! rate-limited Jira asks, longer after each failure in a row.
//!
//! One sync or write at a time per board ([`JiraLink::lock`]), so a slow read can't
//! merge an older picture over a change just written.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use urushi_core::board::{JiraBoard, MAX_SYNCED_ISSUES, SyncState};
use urushi_core::{Board, BoardId, ColumnId, TaskId};

use super::{Service, ServiceError, ServiceResult, State, blocking, lock, new_uuid, now};
use crate::jira::{
    Account, BoardConfig, BoardInfo, CredentialStore, IssueQuery, JiraApi, JiraError, site_address,
};

/// How often boards are read while a client watches.
pub const ACTIVE_INTERVAL: Duration = Duration::from_secs(15);
/// How often boards are read while no client watches.
pub const IDLE_INTERVAL: Duration = Duration::from_secs(120);
/// How often a board is read completely.
pub const FULL_INTERVAL: Duration = Duration::from_secs(300);
/// How often the sync loop looks for boards that are due.
const TICK: Duration = Duration::from_secs(5);
/// What a board says while the daemon has no sign-in.
pub const NOT_SIGNED_IN: &str = "Not signed in to Jira";

/// The service's Jira state.
pub(crate) struct JiraLink {
    pub(super) api: Arc<dyn JiraApi>,
    store: CredentialStore,
    account: Mutex<Option<Account>>,
    boards: Mutex<HashMap<BoardId, BoardSync>>,
}

/// Who the daemon is signed in to Jira as (never the token).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraSignIn {
    /// The site's address.
    pub site: String,
    /// The account's email.
    pub email: String,
    /// The account's name in Jira.
    pub display_name: String,
}

/// How syncing one board is going.
#[derive(Default)]
struct BoardSync {
    lock: Arc<tokio::sync::Mutex<()>>,
    /// The board's configuration, from its last full read.
    config: Option<BoardConfig>,
    /// When the last successful read started.
    last_read: Option<Instant>,
    /// When the last successful full read started.
    last_full: Option<Instant>,
    /// Not before this, after a failure.
    wait_until: Option<Instant>,
    /// Failures in a row.
    failures: u32,
}

impl JiraLink {
    pub(super) fn new(api: Arc<dyn JiraApi>, home: &std::path::Path) -> Self {
        let store = CredentialStore::in_home(home);
        let account = store.load();
        Self {
            api,
            store,
            account: Mutex::new(account),
            boards: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn account(&self) -> Option<Account> {
        lock(&self.account).clone()
    }

    /// The lock that keeps syncs and writes of a board one at a time.
    pub(super) fn lock(&self, board: BoardId) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.boards).entry(board).or_default().lock.clone()
    }
}

/// When to try again after `error`, the `failures`-th in a row.
fn backoff(error: &JiraError, failures: u32) -> Duration {
    let doubled = |base: u64, max: u64| {
        Duration::from_secs(base.saturating_mul(1 << failures.min(10)).min(max))
    };
    match error {
        JiraError::RateLimited {
            retry_after: Some(wait),
        } => (*wait).clamp(Duration::from_secs(60), Duration::from_secs(3600)),
        JiraError::RateLimited { retry_after: None } => doubled(300, 3600),
        JiraError::Unauthorized | JiraError::Forbidden(_) => Duration::from_secs(600),
        _ => doubled(30, 600),
    }
}

/// How a Jira failure reaches clients.
impl From<JiraError> for ServiceError {
    fn from(e: JiraError) -> Self {
        match e {
            JiraError::Unauthorized | JiraError::Forbidden(_) => Self::Forbidden(e.to_string()),
            JiraError::NotFound(_) => Self::NotFound(e.to_string().replace("Jira can't find ", "")),
            JiraError::Rejected(_) => Self::Invalid(e.to_string()),
            JiraError::RateLimited { .. } | JiraError::Unavailable(_) => {
                Self::Unavailable(e.to_string())
            }
        }
    }
}

impl State {
    /// The Jira board that `board` follows.
    pub(super) fn jira_board(&self, board: BoardId) -> ServiceResult<JiraBoard> {
        self.boards
            .iter()
            .find(|b| b.id == board)
            .ok_or_else(|| ServiceError::NotFound(format!("board {board}")))?
            .jira()
            .cloned()
            .ok_or_else(|| ServiceError::Invalid("that board doesn't follow Jira".into()))
    }
}

impl Service {
    /// Who the daemon is signed in to Jira as.
    pub fn jira_sign_in(&self) -> Option<JiraSignIn> {
        self.jira.account().map(|a| JiraSignIn {
            site: a.site,
            email: a.email,
            display_name: a.display_name,
        })
    }

    /// The sign-in, or why a Jira change can't be made without one.
    pub(super) fn jira_account(&self) -> ServiceResult<Account> {
        self.jira
            .account()
            .ok_or_else(|| ServiceError::Precondition(format!("{NOT_SIGNED_IN}: connect it first")))
    }

    /// Signs in to a Jira Cloud site, after Jira accepts the email and token, and keeps
    /// the sign-in for next time.
    pub async fn connect_jira(
        &self,
        site: &str,
        email: &str,
        token: &str,
    ) -> ServiceResult<JiraSignIn> {
        let (email, token) = (email.trim(), token.trim());
        if email.is_empty() || token.is_empty() {
            return Err(ServiceError::Invalid(
                "an email and an API token are needed".into(),
            ));
        }
        let mut account = Account {
            site: site_address(site).map_err(ServiceError::Invalid)?,
            email: email.to_owned(),
            token: token.to_owned(),
            display_name: String::new(),
        };
        let api = self.jira.api.clone();
        let checking = account.clone();
        account.display_name = blocking(move || Ok(api.myself(&checking)?)).await?;
        self.jira
            .store
            .save(&account)
            .map_err(|e| ServiceError::Internal(format!("saving the Jira sign-in: {e}")))?;
        tracing::info!(site = %account.site, "signed in to Jira");
        *lock(&self.jira.account) = Some(account);
        // Every board is due again at once.
        for sync in lock(&self.jira.boards).values_mut() {
            sync.wait_until = None;
            sync.last_read = None;
            sync.failures = 0;
        }
        self.jira_sign_in()
            .ok_or_else(|| ServiceError::Internal("the sign-in went away".into()))
    }

    /// Forgets the sign-in. Imported boards stay, saying they aren't syncing.
    pub fn disconnect_jira(&self) -> ServiceResult<()> {
        self.jira
            .store
            .delete()
            .map_err(|e| ServiceError::Internal(format!("forgetting the Jira sign-in: {e}")))?;
        *lock(&self.jira.account) = None;
        self.note_not_signed_in()
    }

    /// Marks every Jira board as not syncing for want of a sign-in.
    fn note_not_signed_in(&self) -> ServiceResult<()> {
        self.mutate_if_changed(|state| {
            let mut changed = false;
            for jira in state.boards.iter_mut().filter_map(Board::jira_mut) {
                if jira.sync.error.as_deref() != Some(NOT_SIGNED_IN) {
                    jira.sync.error = Some(NOT_SIGNED_IN.into());
                    changed = true;
                }
            }
            Ok(changed)
        })
        .map(drop)
    }

    /// The site's boards whose name contains `query`, each with the board here that
    /// already follows it.
    pub async fn list_jira_boards(
        &self,
        query: &str,
    ) -> ServiceResult<Vec<(BoardInfo, Option<BoardId>)>> {
        let account = self.jira_account()?;
        let api = self.jira.api.clone();
        let query = query.to_owned();
        let boards = blocking(move || Ok(api.boards(&account, &query)?)).await?;
        let imported = self.imported_boards(&self.jira_account()?.site)?;
        Ok(boards
            .into_iter()
            .map(|b| {
                let here = imported.get(&b.id).copied();
                (b, here)
            })
            .collect())
    }

    /// Jira board id → the board here following it, on `site`.
    fn imported_boards(&self, site: &str) -> ServiceResult<HashMap<u64, BoardId>> {
        self.read(|state| {
            Ok(state
                .boards
                .iter()
                .filter_map(|b| {
                    let j = b.jira()?;
                    (j.site == site).then_some((j.board_id, b.id))
                })
                .collect())
        })
    }

    /// Makes a board that follows Jira board `jira_board`, with its columns, and starts
    /// reading its issues. A board already imported is answered as it is.
    pub async fn import_jira_board(&self, jira_board: u64) -> ServiceResult<Board> {
        let account = self.jira_account()?;
        if let Some(id) = self.imported_boards(&account.site)?.get(&jira_board) {
            let id = *id;
            return self.read(|state| {
                state
                    .boards
                    .iter()
                    .find(|b| b.id == id)
                    .cloned()
                    .ok_or_else(|| ServiceError::NotFound(format!("board {id}")))
            });
        }
        let api = self.jira.api.clone();
        let reading = account.clone();
        let config = blocking(move || Ok(api.board(&reading, jira_board)?)).await?;
        let board = Board::new_jira(
            BoardId::from_uuid(new_uuid()),
            JiraBoard {
                site: account.site.clone(),
                board_id: config.info.id,
                name: config.info.name.clone(),
                project_key: config.info.project_key.clone(),
                scrum: config.info.scrum,
                rank_field: config.rank_field.clone(),
                sync: SyncState::default(),
            },
            &config.columns,
            now(),
            || ColumnId::from_uuid(new_uuid()),
        )?;
        lock(&self.jira.boards).entry(board.id).or_default().config = Some(config);
        let board = self.mutate(|state| {
            state.boards.push(board.clone());
            Ok(board)
        })?;
        tracing::info!(board = %board.id, jira = jira_board, "imported a Jira board");
        let me = self.me.clone();
        let id = board.id;
        tokio::spawn(async move {
            if let Some(service) = me.upgrade()
                && let Err(e) = service.sync_board(id, true).await
            {
                tracing::warn!(board = %id, error = %e, "first sync failed");
            }
        });
        Ok(board)
    }

    /// Reads a Jira board and merges what changed: completely when `full` (or when due),
    /// otherwise the issues updated since the last read.
    pub async fn sync_board(&self, board: BoardId, full: bool) -> ServiceResult<()> {
        let guard = self.jira.lock(board);
        let _one_at_a_time = guard.lock().await;
        let jira = self.read(|state| state.jira_board(board))?;
        let Some(account) = self.jira.account() else {
            self.note_not_signed_in()?;
            return Err(ServiceError::Precondition(NOT_SIGNED_IN.into()));
        };
        let (cached, last_read, last_full) = {
            let boards = lock(&self.jira.boards);
            let sync = boards.get(&board);
            (
                sync.and_then(|s| s.config.clone()),
                sync.and_then(|s| s.last_read),
                sync.and_then(|s| s.last_full),
            )
        };
        let full = full
            || cached.is_none()
            || last_read.is_none()
            || last_full.is_none_or(|t| t.elapsed() >= FULL_INTERVAL);
        let started = Instant::now();
        let since = last_read.map(|t| t.elapsed().as_secs() / 60 + 2);
        let api = self.jira.api.clone();
        let read = move || {
            let config = match cached {
                Some(config) if !full => config,
                _ => api.board(&account, jira.board_id)?,
            };
            let fetched = api.issues(
                &account,
                &IssueQuery {
                    filter_id: config.filter_id,
                    sub_query: config.sub_query.clone(),
                    scrum: config.info.scrum,
                    rank_field: config.rank_field.clone(),
                    updated_within_minutes: if full { None } else { since },
                    limit: MAX_SYNCED_ISSUES,
                },
            )?;
            Ok::<_, JiraError>((config, fetched))
        };
        let (config, fetched) = match blocking(move || Ok(read())).await? {
            Ok(read) => read,
            Err(e) => {
                self.sync_failed(board, &e);
                return Err(e.into());
            }
        };
        self.mutate_if_changed(|state| {
            let Some(b) = state.boards.iter_mut().find(|b| b.id == board) else {
                return Ok(false); // deleted meanwhile
            };
            let before = b.clone();
            if full {
                b.set_jira_columns(&config.columns, || ColumnId::from_uuid(new_uuid()))?;
            }
            let report = b.merge_issues(fetched.issues, full, || TaskId::from_uuid(new_uuid()));
            if let Some(j) = b.jira_mut() {
                j.rank_field.clone_from(&config.rank_field);
                j.project_key.clone_from(&config.info.project_key);
                j.sync.error = None;
                if full {
                    j.sync.dropped = report.dropped.max(u32::from(fetched.more));
                }
            }
            let changed = *b != before;
            if (full || changed)
                && let Some(j) = b.jira_mut()
            {
                j.sync.synced_at = Some(now());
            }
            Ok(full || changed)
        })?;
        let mut boards = lock(&self.jira.boards);
        let sync = boards.entry(board).or_default();
        sync.config = Some(config);
        sync.last_read = Some(started);
        if full {
            sync.last_full = Some(started);
        }
        sync.failures = 0;
        sync.wait_until = None;
        Ok(())
    }

    /// Records a failed sync on the board, and when to try again.
    fn sync_failed(&self, board: BoardId, error: &JiraError) {
        tracing::debug!(%board, %error, "Jira sync failed");
        {
            let mut boards = lock(&self.jira.boards);
            let sync = boards.entry(board).or_default();
            sync.wait_until = Some(Instant::now() + backoff(error, sync.failures));
            sync.failures = sync.failures.saturating_add(1);
        }
        let message = error.to_string();
        let _ = self.mutate_if_changed(|state| {
            let Some(j) = state
                .boards
                .iter_mut()
                .find(|b| b.id == board)
                .and_then(Board::jira_mut)
            else {
                return Ok(false);
            };
            let changed = j.sync.error.as_deref() != Some(message.as_str());
            j.sync.error = Some(message);
            Ok(changed)
        });
    }

    /// Like [`Service::mutate`], but persists and wakes clients only when `f` says it
    /// changed something.
    pub(super) fn mutate_if_changed(
        &self,
        f: impl FnOnce(&mut State) -> ServiceResult<bool>,
    ) -> ServiceResult<bool> {
        let changed = {
            let mut state = lock(&self.state);
            let changed = f(&mut state)?;
            if changed {
                state.revision += 1;
            }
            changed
        };
        if changed {
            self.persist()?;
        }
        Ok(changed)
    }

    /// The Jira boards due for a read now.
    fn due_boards(&self) -> Vec<BoardId> {
        let watched = self.changes.receiver_count() > 0;
        let interval = if watched {
            ACTIVE_INTERVAL
        } else {
            IDLE_INTERVAL
        };
        let ids: Vec<BoardId> = lock(&self.state)
            .boards
            .iter()
            .filter(|b| b.jira().is_some())
            .map(|b| b.id)
            .collect();
        let mut boards = lock(&self.jira.boards);
        boards.retain(|id, _| ids.contains(id));
        let now = Instant::now();
        ids.into_iter()
            .filter(|id| {
                let Some(sync) = boards.get(id) else {
                    return true;
                };
                sync.wait_until.is_none_or(|t| now >= t)
                    && sync.last_read.is_none_or(|t| now - t >= interval)
            })
            .collect()
    }

    /// Keeps Jira boards in sync while the daemon runs.
    pub(super) fn keep_jira_synced(&self) {
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some(service) = me.upgrade() else { break };
                if service.jira.account().is_none() {
                    let _ = service.note_not_signed_in();
                    continue;
                }
                for board in service.due_boards() {
                    if let Err(e) = service.sync_board(board, false).await {
                        tracing::debug!(%board, error = %e, "sync failed");
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_back_off_as_asked_and_then_longer() {
        let limited = |s| JiraError::RateLimited {
            retry_after: Some(Duration::from_secs(s)),
        };
        assert_eq!(backoff(&limited(5), 0), Duration::from_secs(60));
        assert_eq!(backoff(&limited(120), 3), Duration::from_secs(120));
        assert_eq!(backoff(&limited(99_999), 0), Duration::from_secs(3600));
        let busy = JiraError::RateLimited { retry_after: None };
        assert_eq!(backoff(&busy, 0), Duration::from_secs(300));
        assert_eq!(backoff(&busy, 1), Duration::from_secs(600));
        assert_eq!(backoff(&busy, 30), Duration::from_secs(3600));
        let down = JiraError::Unavailable(String::new());
        assert_eq!(backoff(&down, 0), Duration::from_secs(30));
        assert_eq!(backoff(&down, 9), Duration::from_secs(600));
    }
}
