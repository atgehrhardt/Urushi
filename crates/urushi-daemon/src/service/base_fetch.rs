//! Fetching a new thread's base once, shared between whoever needs it (ADR 0054).
//!
//! A new thread fetches its base's branch from the remote before its worktree is made
//! (ADR 0028), and that fetch is usually the slowest part of creating a thread. The
//! GUI asks for the fetch as soon as the New Thread dialog opens, so it can run while the
//! user types a title. [`BaseFetches`] makes the two meet:
//! - a fetch that **succeeded** and started at most [`FRESH_FOR`] ago is reused;
//! - a fetch still running is **joined**, and whatever it ends with is shared;
//! - a fetch that **failed** is never reused afterwards, so the create makes its own
//!   attempt (and falls back to the local commit if that fails too).
//!
//! Callers run on blocking threads (git is blocking), so waiting uses a `Condvar`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::lock;
use crate::git::GitError;

/// How long a successful fetch stays good enough for a new thread: long enough to cover
/// typing a title, short enough that the base is still fresh.
pub(crate) const FRESH_FOR: Duration = Duration::from_secs(60);
/// The longest a caller waits for someone else's fetch. Above git's own fetch timeout
/// (30 s), so it only matters if that one fails to fire.
const JOIN_LIMIT: Duration = Duration::from_secs(35);
/// Finished fetches kept at most (the oldest go first).
const MAX_ENTRIES: usize = 64;
/// Background fetches running at once at most; more requests are dropped.
const MAX_PREFETCHES: usize = 4;

/// One branch of one remote, as seen from one repository.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FetchKey {
    /// The repository's root.
    pub repo: PathBuf,
    /// The remote's name.
    pub remote: String,
    /// The branch on the remote.
    pub branch: String,
}

/// How a caller came by its fetch, for logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FetchOrigin {
    /// A recent successful fetch was used.
    Reused,
    /// A fetch already running was waited for.
    Joined,
    /// This caller fetched.
    Own,
}

impl FetchOrigin {
    /// A word for logs.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Reused => "reused",
            Self::Joined => "joined",
            Self::Own => "fetched",
        }
    }
}

enum Entry {
    InFlight {
        id: u64,
    },
    Done {
        id: u64,
        started: Instant,
        result: Result<(), GitError>,
    },
}

#[derive(Default)]
struct Entries {
    next_id: u64,
    map: HashMap<FetchKey, Entry>,
}

/// Fetches of bases, shared while they run and reused for a while after they succeed.
pub(crate) struct BaseFetches {
    entries: Mutex<Entries>,
    done: Condvar,
    prefetching: AtomicUsize,
    fresh_for: Duration,
}

impl BaseFetches {
    /// Fetches reused for `fresh_for` after they started.
    pub(crate) fn new(fresh_for: Duration) -> Self {
        Self {
            entries: Mutex::default(),
            done: Condvar::new(),
            prefetching: AtomicUsize::new(0),
            fresh_for,
        }
    }

    /// Makes sure `key` was fetched recently: reuses a fresh successful fetch, joins one
    /// that is running, or calls `run` to fetch. Blocks; call it from a blocking thread.
    pub(crate) fn fetch(
        &self,
        key: &FetchKey,
        run: impl FnOnce() -> Result<(), GitError>,
    ) -> Result<FetchOrigin, GitError> {
        let deadline = Instant::now() + JOIN_LIMIT;
        let mut entries = lock(&self.entries);
        let id = loop {
            self.prune(&mut entries);
            match entries.map.get(key) {
                Some(Entry::Done {
                    started,
                    result: Ok(()),
                    ..
                }) if started.elapsed() <= self.fresh_for => return Ok(FetchOrigin::Reused),
                Some(Entry::InFlight { id }) => {
                    let id = *id;
                    let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                        return Err(GitError::TimedOut(format!(
                            "fetch {}/{} (waiting for another fetch)",
                            key.remote, key.branch
                        )));
                    };
                    entries = self
                        .done
                        .wait_timeout(entries, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                    if let Some(Entry::Done {
                        id: finished,
                        result,
                        ..
                    }) = entries.map.get(key)
                        && *finished == id
                    {
                        return result.clone().map(|()| FetchOrigin::Joined);
                    }
                }
                // Nothing, a failure, or too old: fetch.
                _ => {
                    entries.next_id += 1;
                    let id = entries.next_id;
                    entries.map.insert(key.clone(), Entry::InFlight { id });
                    break id;
                }
            }
        };
        drop(entries);
        let started = Instant::now();
        let mut guard = InFlight {
            fetches: self,
            key,
            id,
        };
        let result = run();
        guard.finish(started, result.clone());
        result.map(|()| FetchOrigin::Own)
    }

    /// A place for one background fetch, or `None` when enough are running already.
    pub(crate) fn prefetch_slot(self: &Arc<Self>) -> Option<PrefetchSlot> {
        self.prefetching
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_PREFETCHES).then_some(n + 1)
            })
            .ok()
            .map(|_| PrefetchSlot {
                fetches: Arc::clone(self),
            })
    }

    /// Drops finished fetches too old to reuse, then the oldest beyond [`MAX_ENTRIES`].
    /// Running fetches are never dropped.
    fn prune(&self, entries: &mut Entries) {
        let fresh_for = self.fresh_for;
        entries.map.retain(|_, e| match e {
            Entry::InFlight { .. } => true,
            Entry::Done { started, .. } => started.elapsed() <= fresh_for,
        });
        while entries.map.len() > MAX_ENTRIES {
            let oldest = entries
                .map
                .iter()
                .filter_map(|(k, e)| match e {
                    Entry::Done { started, .. } => Some((k.clone(), *started)),
                    Entry::InFlight { .. } => None,
                })
                .min_by_key(|(_, started)| *started)
                .map(|(k, _)| k);
            let Some(oldest) = oldest else { break };
            entries.map.remove(&oldest);
        }
    }
}

/// A fetch this caller runs. Records how it ended and wakes those waiting for it; if
/// the fetch panicked, it records a failure, so nobody waits in vain.
struct InFlight<'a> {
    fetches: &'a BaseFetches,
    key: &'a FetchKey,
    id: u64,
}

impl InFlight<'_> {
    fn finish(&mut self, started: Instant, result: Result<(), GitError>) {
        let mut entries = lock(&self.fetches.entries);
        entries.map.insert(
            self.key.clone(),
            Entry::Done {
                id: self.id,
                started,
                result,
            },
        );
        self.id = 0; // finished: nothing for `drop` to do
        self.fetches.done.notify_all();
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if self.id == 0 {
            return;
        }
        let mut entries = lock(&self.fetches.entries);
        let failed = Err(GitError::Failed {
            command: format!("fetch {} {}", self.key.remote, self.key.branch),
            message: "the fetch stopped unexpectedly".into(),
        });
        entries.map.insert(
            self.key.clone(),
            Entry::Done {
                id: self.id,
                started: Instant::now(),
                result: failed,
            },
        );
        self.fetches.done.notify_all();
    }
}

/// A claim on one of the [`MAX_PREFETCHES`] background fetches; released on drop.
pub(crate) struct PrefetchSlot {
    fetches: Arc<BaseFetches>,
}

impl PrefetchSlot {
    /// The fetches this slot belongs to.
    pub(crate) fn fetches(&self) -> &BaseFetches {
        &self.fetches
    }
}

impl Drop for PrefetchSlot {
    fn drop(&mut self) {
        self.fetches.prefetching.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[path = "base_fetch_tests.rs"]
mod tests;
