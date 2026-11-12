//! Saving the workspace document from one thread, coalescing bursts (ADR 0053).
//!
//! Every change used to serialize and fsync the whole document before it was announced
//! and answered, and concurrent changes queued one save each. Now a change asks the
//! [`Saver`] for its revision and waits for it. The saver's thread takes a snapshot of
//! the *newest* state when a save starts, so every change made while the previous save
//! ran is covered by one write. A reply still goes out only once its change is on disk.
//!
//! The saver is a plain OS thread rather than a tokio task, so it keeps saving under
//! single-threaded runtimes (tests), where a waiting request blocks the only worker.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use thiserror::Error;

use super::lock;
use crate::store::{Document, Store, StoreError};

/// Why a change couldn't be confirmed as saved.
#[derive(Clone, Debug, Error)]
pub enum SaveError {
    /// Writing the document failed. The change is in memory; the next change retries.
    #[error("saving the workspace failed: {0}")]
    Failed(Arc<StoreError>),
    /// The saver has stopped (the daemon is shutting down) before saving the change.
    #[error("the daemon is stopping")]
    Closed,
}

/// The newest document and its revision, taken when a save starts.
pub(crate) type Snapshot = Box<dyn Fn() -> (Document, u64) + Send>;

/// Saves the newest revision of the workspace on its own thread, on request.
pub(crate) struct Saver {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    progress: Mutex<Progress>,
    /// Wakes the saver thread: a revision was requested, or it should close.
    work: Condvar,
    /// Wakes waiters: a save finished or failed, or the saver closed.
    done: Condvar,
}

#[derive(Default)]
struct Progress {
    /// The newest revision a change asked to be saved.
    wanted: Option<u64>,
    /// The newest revision on disk.
    saved: Option<u64>,
    /// The revision the last attempt covered, and why it failed. Cleared by a later
    /// successful save.
    failed: Option<(u64, Arc<StoreError>)>,
    /// Stop once nothing is left to save.
    closing: bool,
    /// The thread has stopped; nothing more will be saved.
    closed: bool,
}

impl Progress {
    /// Whether a requested revision still needs an attempt.
    fn pending(&self) -> bool {
        self.wanted.is_some_and(|wanted| {
            self.saved.is_none_or(|saved| wanted > saved)
                && self
                    .failed
                    .as_ref()
                    .is_none_or(|(failed, _)| wanted > *failed)
        })
    }
}

impl Saver {
    /// Starts the saver's thread. `snapshot` returns the current document and revision;
    /// it is called once per save.
    pub(crate) fn spawn(store: Arc<dyn Store>, snapshot: Snapshot) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            progress: Mutex::new(Progress::default()),
            work: Condvar::new(),
            done: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("urushi-saver".into())
            .spawn(move || worker.run(&*store, &*snapshot))?;
        Ok(Self {
            shared,
            thread: Mutex::new(Some(thread)),
        })
    }

    /// Asks for `revision` (or newer) to be saved. Never blocks on the save.
    pub(crate) fn request(&self, revision: u64) {
        let mut progress = lock(&self.shared.progress);
        progress.wanted = Some(progress.wanted.map_or(revision, |w| w.max(revision)));
        self.shared.work.notify_one();
    }

    /// Blocks until `revision` (or newer) is on disk, or the attempt that covered it
    /// failed, or the saver closed without saving it.
    pub(crate) fn wait(&self, revision: u64) -> Result<(), SaveError> {
        let mut progress = lock(&self.shared.progress);
        loop {
            if progress.saved.is_some_and(|saved| saved >= revision) {
                return Ok(());
            }
            if let Some((failed, error)) = &progress.failed
                && *failed >= revision
            {
                return Err(SaveError::Failed(Arc::clone(error)));
            }
            if progress.closed {
                return Err(SaveError::Closed);
            }
            progress = self
                .shared
                .done
                .wait(progress)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Saves what is still pending, then stops. Waits at most `limit`; returns whether
    /// the saver stopped in time. Later requests are never saved.
    pub(crate) fn close(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut progress = lock(&self.shared.progress);
        progress.closing = true;
        self.shared.work.notify_one();
        while !progress.closed {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            progress = self
                .shared
                .done
                .wait_timeout(progress, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        drop(progress);
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
        true
    }
}

impl Drop for Saver {
    fn drop(&mut self) {
        // Let the thread finish what's pending and end on its own; never join here.
        lock(&self.shared.progress).closing = true;
        self.shared.work.notify_one();
    }
}

impl Shared {
    fn run(&self, store: &dyn Store, snapshot: &dyn Fn() -> (Document, u64)) {
        loop {
            {
                let mut progress = lock(&self.progress);
                while !progress.pending() && !progress.closing {
                    progress = self
                        .work
                        .wait(progress)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if !progress.pending() {
                    progress.closed = true;
                    self.done.notify_all();
                    return;
                }
            }
            let started = Instant::now();
            let (document, revision) = snapshot();
            let snapshot_ms = started.elapsed().as_millis();
            let result = store.save(&document);
            let mut progress = lock(&self.progress);
            match result {
                Ok(()) => {
                    let previous = progress.saved;
                    progress.saved = Some(previous.map_or(revision, |s| s.max(revision)));
                    if progress
                        .failed
                        .as_ref()
                        .is_some_and(|(failed, _)| *failed <= revision)
                    {
                        progress.failed = None;
                    }
                    tracing::debug!(
                        revision,
                        coalesced = previous.map_or(1, |p| revision.saturating_sub(p)),
                        snapshot_ms,
                        save_ms = started.elapsed().as_millis(),
                        "saved the workspace"
                    );
                }
                Err(e) => {
                    tracing::error!(revision, error = %e, "saving the workspace failed");
                    progress.failed = Some((revision, Arc::new(e)));
                }
            }
            self.done.notify_all();
        }
    }
}

#[cfg(test)]
#[path = "saver_tests.rs"]
mod tests;
