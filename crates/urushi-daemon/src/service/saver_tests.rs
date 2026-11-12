#![allow(clippy::unwrap_used, reason = "tests")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;

use super::*;

const LONG: Duration = Duration::from_secs(10);

/// A store that records which revision each save wrote, and can hold saves until the
/// test lets them through, or fail them.
struct FakeStore {
    /// The revision the last snapshot was taken at.
    taken: Arc<AtomicU64>,
    saved: Mutex<Vec<u64>>,
    gated: AtomicBool,
    fail: AtomicBool,
    entered: Mutex<mpsc::Sender<()>>,
    gate: Mutex<mpsc::Receiver<()>>,
}

impl Store for FakeStore {
    fn load(&self) -> Result<Document, StoreError> {
        Ok(Document::default())
    }

    fn save(&self, _document: &Document) -> Result<(), StoreError> {
        let _ = lock(&self.entered).send(());
        if self.gated.load(Ordering::SeqCst) {
            lock(&self.gate).recv_timeout(LONG).unwrap();
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(StoreError::Io {
                path: "workspace.json".into(),
                source: std::io::Error::other("disk full"),
            });
        }
        lock(&self.saved).push(self.taken.load(Ordering::SeqCst));
        Ok(())
    }
}

struct Fixture {
    saver: Saver,
    store: Arc<FakeStore>,
    /// The workspace's current revision, as the snapshot sees it.
    current: Arc<AtomicU64>,
    entered: mpsc::Receiver<()>,
    open: mpsc::Sender<()>,
}

impl Fixture {
    fn new(gated: bool) -> Self {
        let (entered_tx, entered) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let current = Arc::new(AtomicU64::new(0));
        let taken = Arc::new(AtomicU64::new(0));
        let store = Arc::new(FakeStore {
            taken: Arc::clone(&taken),
            saved: Mutex::default(),
            gated: AtomicBool::new(gated),
            fail: AtomicBool::new(false),
            entered: Mutex::new(entered_tx),
            gate: Mutex::new(gate),
        });
        let now = Arc::clone(&current);
        let snapshot: Snapshot = Box::new(move || {
            let revision = now.load(Ordering::SeqCst);
            taken.store(revision, Ordering::SeqCst);
            (Document::default(), revision)
        });
        let saver = Saver::spawn(Arc::clone(&store) as Arc<dyn Store>, snapshot).unwrap();
        Self {
            saver,
            store,
            current,
            entered,
            open,
        }
    }

    /// A change to `revision`: the state moves on, then asks to be saved.
    fn change(&self, revision: u64) {
        self.current.store(revision, Ordering::SeqCst);
        self.saver.request(revision);
    }

    fn saved(&self) -> Vec<u64> {
        lock(&self.store.saved).clone()
    }
}

#[test]
fn a_burst_is_saved_once_after_the_save_in_progress() {
    let f = Fixture::new(true);
    f.change(1);
    f.entered.recv_timeout(LONG).unwrap(); // revision 1 is being written
    for revision in 2..=5 {
        f.change(revision);
    }
    f.open.send(()).unwrap();
    f.open.send(()).unwrap();
    f.saver.wait(5).unwrap();
    assert_eq!(f.saved(), vec![1, 5]);
}

#[test]
fn waiters_return_only_once_their_revision_is_saved() {
    let f = Fixture::new(true);
    f.change(1);
    f.entered.recv_timeout(LONG).unwrap();
    let (returned_tx, returned) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            returned_tx.send(f.saver.wait(1).is_ok()).unwrap();
        });
        assert!(
            returned.recv_timeout(Duration::from_millis(100)).is_err(),
            "returned before the save finished"
        );
        f.open.send(()).unwrap();
        assert!(returned.recv_timeout(LONG).unwrap());
    });
}

#[test]
fn a_failed_save_fails_its_waiters_and_the_next_change_retries() {
    let f = Fixture::new(false);
    f.store.fail.store(true, Ordering::SeqCst);
    f.change(1);
    assert!(matches!(f.saver.wait(1), Err(SaveError::Failed(_))));
    f.store.fail.store(false, Ordering::SeqCst);
    f.change(2);
    f.saver.wait(2).unwrap();
    assert_eq!(f.saved(), vec![2]);
    // Revision 1 is on disk now too, as part of 2.
    f.saver.wait(1).unwrap();
}

#[test]
fn the_first_save_happens_at_revision_zero() {
    let f = Fixture::new(false);
    f.saver.request(0);
    f.saver.wait(0).unwrap();
    assert_eq!(f.saved(), vec![0]);
}

#[test]
fn closing_saves_whats_pending_then_refuses() {
    let f = Fixture::new(true);
    f.change(1);
    f.entered.recv_timeout(LONG).unwrap();
    f.change(3);
    f.open.send(()).unwrap();
    f.open.send(()).unwrap();
    assert!(f.saver.close(LONG));
    assert_eq!(f.saved(), vec![1, 3]);
    f.change(4);
    assert!(matches!(f.saver.wait(4), Err(SaveError::Closed)));
    assert_eq!(f.saved(), vec![1, 3]);
}

#[test]
fn closing_gives_up_after_its_limit() {
    let f = Fixture::new(true);
    f.change(1);
    f.entered.recv_timeout(LONG).unwrap();
    assert!(!f.saver.close(Duration::from_millis(50)));
    f.open.send(()).unwrap();
    f.saver.wait(1).unwrap();
}
