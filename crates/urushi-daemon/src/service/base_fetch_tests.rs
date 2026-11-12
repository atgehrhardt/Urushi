#![allow(clippy::unwrap_used, clippy::panic, reason = "tests")]

use std::sync::atomic::AtomicU32;
use std::sync::mpsc;

use super::*;

fn key(branch: &str) -> FetchKey {
    FetchKey {
        repo: PathBuf::from("/repo"),
        remote: "origin".into(),
        branch: branch.into(),
    }
}

fn failure() -> GitError {
    GitError::Failed {
        command: "fetch".into(),
        message: "offline".into(),
    }
}

#[test]
fn a_fresh_success_is_reused_and_an_old_one_fetched_again() {
    let fetches = BaseFetches::new(FRESH_FOR);
    let runs = AtomicU32::new(0);
    let run = || {
        runs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    assert_eq!(fetches.fetch(&key("main"), run).unwrap(), FetchOrigin::Own);
    assert_eq!(
        fetches.fetch(&key("main"), run).unwrap(),
        FetchOrigin::Reused
    );
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    let stale = BaseFetches::new(Duration::ZERO);
    stale.fetch(&key("main"), run).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(stale.fetch(&key("main"), run).unwrap(), FetchOrigin::Own);
    assert_eq!(runs.load(Ordering::SeqCst), 3);
}

#[test]
fn callers_at_once_share_one_fetch() {
    let fetches = &BaseFetches::new(FRESH_FOR);
    let runs = &AtomicU32::new(0);
    let (started_tx, started) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let first = scope.spawn(move || {
            fetches.fetch(&key("main"), || {
                runs.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                released.recv().unwrap();
                Ok(())
            })
        });
        started.recv().unwrap();
        let second = scope.spawn(move || fetches.fetch(&key("main"), || panic!("fetched twice")));
        std::thread::sleep(Duration::from_millis(50));
        release.send(()).unwrap();
        assert_eq!(first.join().unwrap().unwrap(), FetchOrigin::Own);
        assert_eq!(second.join().unwrap().unwrap(), FetchOrigin::Joined);
    });
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[test]
fn a_joined_failure_is_shared_but_never_reused_later() {
    let fetches = &BaseFetches::new(FRESH_FOR);
    let (started_tx, started) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let first = scope.spawn(move || {
            fetches.fetch(&key("main"), || {
                started_tx.send(()).unwrap();
                released.recv().unwrap();
                Err(failure())
            })
        });
        started.recv().unwrap();
        let second = scope.spawn(move || fetches.fetch(&key("main"), || panic!("fetched twice")));
        std::thread::sleep(Duration::from_millis(50));
        release.send(()).unwrap();
        assert!(first.join().unwrap().is_err());
        assert!(second.join().unwrap().is_err());
    });
    // Once it has failed, the next caller tries again.
    assert_eq!(
        fetches.fetch(&key("main"), || Ok(())).unwrap(),
        FetchOrigin::Own
    );
}

#[test]
fn different_branches_do_not_share() {
    let fetches = BaseFetches::new(FRESH_FOR);
    fetches.fetch(&key("main"), || Ok(())).unwrap();
    assert_eq!(
        fetches.fetch(&key("dev"), || Ok(())).unwrap(),
        FetchOrigin::Own
    );
}

#[test]
fn finished_fetches_are_capped() {
    let fetches = BaseFetches::new(FRESH_FOR);
    for i in 0..(MAX_ENTRIES + 10) {
        fetches.fetch(&key(&format!("b{i}")), || Ok(())).unwrap();
    }
    // The next look prunes down to the cap.
    fetches.fetch(&key("last"), || Ok(())).unwrap();
    assert!(lock(&fetches.entries).map.len() <= MAX_ENTRIES + 1);
    // The newest are kept.
    assert_eq!(
        fetches.fetch(&key("last"), || Ok(())).unwrap(),
        FetchOrigin::Reused
    );
}

#[test]
fn a_fetch_that_panics_releases_whoever_waits() {
    let fetches = &BaseFetches::new(FRESH_FOR);
    let (started_tx, started) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let first = scope.spawn(move || {
            fetches.fetch(&key("main"), || {
                started_tx.send(()).unwrap();
                released.recv().unwrap();
                panic!("git went away");
            })
        });
        started.recv().unwrap();
        let second = scope.spawn(move || fetches.fetch(&key("main"), || panic!("fetched twice")));
        std::thread::sleep(Duration::from_millis(50));
        release.send(()).unwrap();
        assert!(first.join().is_err());
        assert!(second.join().unwrap().is_err());
    });
}

#[test]
fn background_fetches_are_capped() {
    let fetches = Arc::new(BaseFetches::new(FRESH_FOR));
    let slots: Vec<_> = (0..MAX_PREFETCHES)
        .map(|_| fetches.prefetch_slot().unwrap())
        .collect();
    assert!(fetches.prefetch_slot().is_none());
    drop(slots);
    assert!(fetches.prefetch_slot().is_some());
}
