//! Keeping Jira boards in sync (ADR 0023, after ADR 0020): near-real-time for a board
//! someone looks at, cheap for the rest.
//!
//! A board is read:
//! - every [`WATCHED_INTERVAL`] while a connection shows it (`WatchBoard`), every
//!   [`ACTIVE_INTERVAL`] while a client watches the workspace, and every
//!   [`IDLE_INTERVAL`] otherwise, and at once after a change is written to it;
//! - cheaply: Jira is first asked only *which* issues changed lately (ids and update
//!   times), and only those whose time differs from the board's are read in full;
//! - completely on import, on `SyncBoard`, and every [`FULL_INTERVAL`]: the board's
//!   configuration (columns) and every issue, so deleted and moved-away issues go.
//!
//! Boards sync side by side, each under its own lock (one sync or write at a time, so a
//! slow read can't merge an older picture over a change just written). Reads happen off
//! the state lock; a merge that changes nothing wakes no client. Failures are recorded
//! on the board for clients to show, and back off: as long as a rate-limited Jira asks,
//! longer after each failure in a row.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use urushi_core::{Board, BoardId, ColumnId, TaskId, Timestamp};

use super::jira::{NOT_SIGNED_IN, board_query};
use super::{Service, ServiceError, ServiceResult, State, blocking, lock, new_uuid, now};
use crate::jira::{BoardConfig, JiraError, Me};

/// How often a board someone looks at is read.
pub const WATCHED_INTERVAL: Duration = Duration::from_secs(3);
/// How often boards are read while a client watches the workspace.
pub const ACTIVE_INTERVAL: Duration = Duration::from_secs(15);
/// How often boards are read while no client watches.
pub const IDLE_INTERVAL: Duration = Duration::from_secs(120);
/// How often a board is read completely.
pub const FULL_INTERVAL: Duration = Duration::from_secs(300);
/// How often the sync loop looks for boards that are due.
const TICK: Duration = Duration::from_secs(1);

/// How syncing one board is going.
#[derive(Default)]
pub(crate) struct BoardSync {
    pub(super) lock: Arc<tokio::sync::Mutex<()>>,
    /// The board's configuration, from its last full read.
    pub(super) config: Option<BoardConfig>,
    /// When the last successful read started.
    last_read: Option<Instant>,
    /// When the last successful full read started.
    last_full: Option<Instant>,
    /// Not before this, after a failure.
    pub(super) wait_until: Option<Instant>,
    /// Failures in a row.
    pub(super) failures: u32,
    /// Due now, whatever the interval (a change was just written, or someone opened it).
    poked: bool,
    /// A sync of it is under way.
    running: bool,
    /// When each issue read for it (epics too) last changed, by issue id: what a look
    /// at what changed compares with.
    seen: HashMap<String, Timestamp>,
}

impl BoardSync {
    /// Forgets the last read, so the board is read again at once (and in full).
    pub(super) fn reset(&mut self) {
        self.wait_until = None;
        self.last_read = None;
        self.failures = 0;
    }
}

/// While kept, a connection shows a Jira board: it is read every
/// [`WATCHED_INTERVAL`]. Dropping it (another board, or the connection closing)
/// stops that.
pub struct BoardWatch {
    board: BoardId,
    watched: Arc<Mutex<HashMap<BoardId, usize>>>,
}

impl Drop for BoardWatch {
    fn drop(&mut self) {
        let mut watched = lock(&self.watched);
        if let Some(n) = watched.get_mut(&self.board) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                watched.remove(&self.board);
            }
        }
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

/// How often a board is read: watched by someone, any client connected, or none.
fn interval(watched: bool, subscribed: bool) -> Duration {
    if watched {
        WATCHED_INTERVAL
    } else if subscribed {
        ACTIVE_INTERVAL
    } else {
        IDLE_INTERVAL
    }
}

/// What a read brought back.
struct Read {
    config: BoardConfig,
    issues: Vec<urushi_core::board::SyncedIssue>,
    more: bool,
    me: Option<Me>,
}

impl Service {
    /// Shows `board` on a connection until the watch is dropped; it is read at once.
    pub fn watch_board(&self, board: BoardId) -> ServiceResult<BoardWatch> {
        self.read(|state| state.jira_board(board))?;
        *lock(&self.jira.watched).entry(board).or_default() += 1;
        self.poke(board);
        Ok(BoardWatch {
            board,
            watched: self.jira.watched.clone(),
        })
    }

    /// Makes `board` due now.
    pub(super) fn poke(&self, board: BoardId) {
        lock(&self.jira.boards).entry(board).or_default().poked = true;
    }

    /// Notes when issues just read back after a write last changed, so the next look
    /// doesn't read them again.
    pub(super) fn saw(&self, board: BoardId, issues: &[urushi_core::board::SyncedIssue]) {
        let mut boards = lock(&self.jira.boards);
        let seen = &mut boards.entry(board).or_default().seen;
        seen.extend(issues.iter().map(|i| (i.issue.id.clone(), i.issue.updated)));
    }

    /// Reads a Jira board and merges what changed: completely when `full` (or when due),
    /// otherwise the issues that changed since the last read.
    pub async fn sync_board(&self, board: BoardId, full: bool) -> ServiceResult<()> {
        let guard = self.jira.lock(board);
        let _one_at_a_time = guard.lock().await;
        let jira = self.read(|state| state.jira_board(board))?;
        let Some(account) = self.jira.account() else {
            self.note_not_signed_in()?;
            return Err(ServiceError::Precondition(NOT_SIGNED_IN.into()));
        };
        let (cached, last_read, last_full, known) = {
            let boards = lock(&self.jira.boards);
            let sync = boards.get(&board);
            (
                sync.and_then(|s| s.config.clone()),
                sync.and_then(|s| s.last_read),
                sync.and_then(|s| s.last_full),
                sync.map(|s| s.seen.clone()).unwrap_or_default(),
            )
        };
        let full = full
            || cached.is_none()
            || last_read.is_none()
            || last_full.is_none_or(|t| t.elapsed() >= FULL_INTERVAL);
        let started = Instant::now();
        // Jira's relative times count whole minutes: a minute's margin.
        let since = last_read.map_or(2, |t| t.elapsed().as_secs() / 60 + 2);
        let api = self.jira.api.clone();
        let read = move || {
            let config = match cached {
                Some(config) if !full => config,
                _ => api.board(&account, jira.board_id)?,
            };
            let mut query = board_query(&config);
            let (issues, more) = if full {
                let fetched = api.issues(&account, &query)?;
                (fetched.issues, fetched.more)
            } else {
                // Which changed lately, then only those whose time is new here.
                query.updated_within_minutes = Some(since);
                let changed: Vec<String> = api
                    .changed(&account, &query)?
                    .into_iter()
                    .filter(|(id, updated)| known.get(id) != Some(updated))
                    .map(|(id, _)| id)
                    .collect();
                if changed.is_empty() {
                    (Vec::new(), false)
                } else {
                    query.updated_within_minutes = None;
                    query.limit = changed.len();
                    query.extra_jql = vec![format!("id in ({})", changed.join(", "))];
                    (api.issues(&account, &query)?.issues, false)
                }
            };
            // A sign-in from before account ids were kept learns its own.
            let me = if full && account.account_id.is_empty() {
                api.myself(&account).ok()
            } else {
                None
            };
            Ok::<_, JiraError>(Read {
                config,
                issues,
                more,
                me,
            })
        };
        let read = match blocking(move || Ok(read())).await? {
            Ok(read) => read,
            Err(e) => {
                self.sync_failed(board, &e);
                return Err(e.into());
            }
        };
        self.merge_read(board, full, &read)?;
        let mut boards = lock(&self.jira.boards);
        let sync = boards.entry(board).or_default();
        if full {
            sync.seen.clear();
        }
        sync.seen.extend(
            read.issues
                .iter()
                .map(|i| (i.issue.id.clone(), i.issue.updated)),
        );
        sync.config = Some(read.config);
        sync.last_read = Some(started);
        if full {
            sync.last_full = Some(started);
        }
        sync.failures = 0;
        sync.wait_until = None;
        drop(boards);
        if let Some(me) = read.me {
            self.learn_me(&me);
        }
        Ok(())
    }

    /// Merges what a read brought back into its board.
    fn merge_read(&self, board: BoardId, full: bool, read: &Read) -> ServiceResult<()> {
        let config = &read.config;
        self.mutate_if_changed(|state| {
            let Some(b) = state.boards.iter_mut().find(|b| b.id == board) else {
                return Ok(false); // deleted meanwhile
            };
            let before = b.clone();
            if full {
                b.set_jira_columns(&config.columns, || ColumnId::from_uuid(new_uuid()))?;
            }
            let report =
                b.merge_issues(read.issues.clone(), full, || TaskId::from_uuid(new_uuid()));
            if let Some(j) = b.jira_mut() {
                j.rank_field.clone_from(&config.rank_field);
                j.points_field.clone_from(&config.points_field);
                j.sprint_field.clone_from(&config.sprint_field);
                j.quick_filters.clone_from(&config.quick_filters);
                j.project_key.clone_from(&config.info.project_key);
                j.sync.error = None;
                if full {
                    j.sync.dropped = report.dropped.max(u32::from(read.more));
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
        Ok(())
    }

    /// Keeps the account id (and name) Jira says the sign-in has.
    fn learn_me(&self, me: &Me) {
        let mut account = lock(&self.jira.account);
        let Some(a) = account.as_mut() else {
            return;
        };
        if me.account_id.is_empty() || a.account_id == me.account_id {
            return;
        }
        a.account_id.clone_from(&me.account_id);
        a.display_name.clone_from(&me.display_name);
        if let Err(e) = self.jira.store.save(a) {
            tracing::warn!(error = %e, "couldn't keep the Jira account id");
        }
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

    /// The Jira boards due for a read now (and not being read), marked as being read.
    fn due_boards(&self) -> Vec<BoardId> {
        let subscribed = self.changes.receiver_count() > 0;
        let ids: Vec<BoardId> = lock(&self.state)
            .boards
            .iter()
            .filter(|b| b.jira().is_some())
            .map(|b| b.id)
            .collect();
        let watched = lock(&self.jira.watched).clone();
        let mut boards = lock(&self.jira.boards);
        boards.retain(|id, _| ids.contains(id));
        let now = Instant::now();
        let mut due = Vec::new();
        for id in ids {
            let sync = boards.entry(id).or_default();
            let every = interval(watched.contains_key(&id), subscribed);
            let ready = sync.wait_until.is_none_or(|t| now >= t)
                && (sync.poked || sync.last_read.is_none_or(|t| now - t >= every));
            if ready && !sync.running {
                sync.running = true;
                sync.poked = false;
                due.push(id);
            }
        }
        due
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
                    let me = service.me.clone();
                    // Side by side: a slow board doesn't hold up the others.
                    tokio::spawn(async move {
                        let Some(service) = me.upgrade() else { return };
                        if let Err(e) = service.sync_board(board, false).await {
                            tracing::debug!(%board, error = %e, "sync failed");
                        }
                        if let Some(sync) = lock(&service.jira.boards).get_mut(&board) {
                            sync.running = false;
                        }
                    });
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

    #[test]
    fn a_watched_board_is_read_most_often() {
        assert_eq!(interval(true, true), WATCHED_INTERVAL);
        assert_eq!(interval(true, false), WATCHED_INTERVAL);
        assert_eq!(interval(false, true), ACTIVE_INTERVAL);
        assert_eq!(interval(false, false), IDLE_INTERVAL);
        let watched = Arc::new(Mutex::new(HashMap::new()));
        let board = BoardId::from_u128(1);
        let watch = |w: &Arc<Mutex<HashMap<BoardId, usize>>>| {
            *lock(w).entry(board).or_default() += 1;
            BoardWatch {
                board,
                watched: w.clone(),
            }
        };
        let (a, b) = (watch(&watched), watch(&watched));
        drop(a);
        assert_eq!(lock(&watched).get(&board), Some(&1));
        drop(b);
        assert!(lock(&watched).is_empty(), "released with the last watch");
    }
}
