//! Keeping the pull requests of Jira issues linked to threads current (ADR 0036).
//!
//! A thread card shows the pull request of the issue it was started from. Jira's
//! development info says which those are, at one or two requests per issue, so only
//! issues whose task is linked to a thread are read: one just linked at once, then
//! every [`PR_ACTIVE_INTERVAL`] while a client watches the workspace and every
//! [`PR_IDLE_INTERVAL`] otherwise, at most [`PR_PER_ROUND`] per round. A failure backs off
//! like a board sync, and never marks a board as failing: pull requests are extra.
//! An issue no longer linked loses the pull requests it had, so nothing stale stays.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use urushi_core::BoardId;
use urushi_core::board::PullRequest;

use super::jira::account_for;
use super::jira_sync::backoff;
use super::{Service, ServiceResult, blocking, lock};
use crate::jira::{Account, JiraError};

/// How often a linked issue's pull requests are read while a client watches.
pub const PR_ACTIVE_INTERVAL: Duration = Duration::from_secs(60);
/// How often they are read while no client watches.
pub const PR_IDLE_INTERVAL: Duration = Duration::from_secs(600);
/// Most issues read in one round; the rest wait for the next.
pub const PR_PER_ROUND: usize = 20;
/// How often the loop looks for issues that are due.
const TICK: Duration = Duration::from_secs(5);

/// When each linked issue's pull requests were last read, and the backoff.
#[derive(Default)]
pub(crate) struct DevReads {
    /// By board and Jira issue id (two sites can share an id).
    last: HashMap<(BoardId, String), Instant>,
    wait_until: Option<Instant>,
    failures: u32,
    running: bool,
}

/// What one round read: `(board, issue id, its pull requests)`.
type Found = Vec<(BoardId, String, Vec<PullRequest>)>;

impl Service {
    /// The linked issues due for a read, `(board, issue id)`, and marks a round as
    /// running; `None` when one already runs or waits after a failure.
    fn due_pull_requests(&self) -> Option<Vec<(BoardId, String)>> {
        let subscribed = self.changes.receiver_count() > 0;
        let every = if subscribed {
            PR_ACTIVE_INTERVAL
        } else {
            PR_IDLE_INTERVAL
        };
        let accounts = self.jira.accounts();
        // Only boards a sign-in reads: the others can't be asked.
        let linked: Vec<(BoardId, String)> = lock(&self.state)
            .boards
            .iter()
            .filter(|b| {
                b.jira()
                    .is_some_and(|j| account_for(&accounts, j).is_some())
            })
            .flat_map(|b| b.linked_issue_ids().into_iter().map(move |id| (b.id, id)))
            .collect();
        let mut dev = lock(&self.jira.dev);
        let now = Instant::now();
        if dev.running || dev.wait_until.is_some_and(|t| now < t) {
            return None;
        }
        // Forget unlinked issues, so linking one again reads it at once.
        let ids: HashSet<&(BoardId, String)> = linked.iter().collect();
        dev.last.retain(|at, _| ids.contains(at));
        let mut due: Vec<(BoardId, String)> = linked
            .into_iter()
            .filter(|at| dev.last.get(at).is_none_or(|t| now - *t >= every))
            .collect();
        // Never read first, then the longest ago.
        due.sort_by_key(|at| dev.last.get(at).copied());
        due.truncate(PR_PER_ROUND);
        dev.running = true;
        Some(due)
    }

    /// Reads the pull requests of the linked Jira issues that are due, and keeps what
    /// changed; also clears those of issues no longer linked. Returns how many issues
    /// were read. Each issue is asked about with its board's sign-in; without any, it
    /// does nothing.
    pub async fn read_pull_requests(&self) -> ServiceResult<usize> {
        let accounts = self.jira.accounts();
        if accounts.is_empty() {
            return Ok(0);
        }
        let Some(due) = self.due_pull_requests() else {
            return Ok(0);
        };
        // Each due issue with the sign-in of its board.
        let due: Vec<(BoardId, String, Account)> = {
            let state = lock(&self.state);
            due.into_iter()
                .filter_map(|(board, id)| {
                    let jira = state.boards.iter().find(|b| b.id == board)?.jira()?;
                    let account = account_for(&accounts, jira)?.clone();
                    Some((board, id, account))
                })
                .collect()
        };
        let api = self.jira.api.clone();
        let (found, error) = blocking(move || {
            let mut found: Found = Vec::new();
            for (board, id, account) in due {
                match api.pull_requests(&account, &id) {
                    Ok(prs) => found.push((board, id, prs)),
                    // Stop at the first failure: the rest would most likely fail too.
                    Err(e) => return Ok((found, Some(e))),
                }
            }
            Ok((found, None::<JiraError>))
        })
        .await
        .unwrap_or_else(|e| (Vec::new(), Some(JiraError::Unavailable(e.to_string()))));
        let read = found.len();
        let stored = self.keep_pull_requests(&found);
        let mut dev = lock(&self.jira.dev);
        dev.running = false;
        let now = Instant::now();
        for (board, id, _) in &found {
            dev.last.insert((*board, id.clone()), now);
        }
        if let Some(e) = error {
            tracing::debug!(error = %e, "reading pull requests failed");
            dev.wait_until = Some(now + backoff(&e, dev.failures));
            dev.failures = dev.failures.saturating_add(1);
        } else {
            dev.wait_until = None;
            dev.failures = 0;
        }
        drop(dev);
        stored?;
        Ok(read)
    }

    /// Stores pull requests just read, and clears those of issues no longer linked;
    /// clients wake only if something changed.
    fn keep_pull_requests(&self, found: &Found) -> ServiceResult<bool> {
        self.mutate_if_changed(|state| {
            let mut changed = false;
            for board in state.boards.iter_mut().filter(|b| b.jira().is_some()) {
                let stale: Vec<String> = board
                    .all_tasks()
                    .filter(|t| t.thread.is_none())
                    .filter_map(|t| t.issue.as_ref())
                    .filter(|i| !i.pull_requests.is_empty())
                    .map(|i| i.id.clone())
                    .collect();
                for id in stale {
                    changed |= board.set_pull_requests(&id, Vec::new());
                }
                let here = board.id;
                for (_, id, prs) in found.iter().filter(|(b, ..)| *b == here) {
                    changed |= board.set_pull_requests(id, prs.clone());
                }
            }
            Ok(changed)
        })
    }

    /// Keeps linked issues' pull requests current while the daemon runs.
    pub(super) fn keep_pull_requests_current(&self) {
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some(service) = me.upgrade() else { break };
                if let Err(e) = service.read_pull_requests().await {
                    tracing::debug!(error = %e, "keeping pull requests failed");
                }
            }
        });
    }
}
