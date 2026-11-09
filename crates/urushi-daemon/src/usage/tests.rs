#![allow(clippy::unwrap_used, reason = "test code")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A fixed process table and usage, counting how often each is read. Scripted answers,
/// if any, come before the fixed usage.
struct Fake {
    table: &'static str,
    reads: AtomicUsize,
    listings: AtomicUsize,
    script: Mutex<VecDeque<Result<Usage, Failure>>>,
}

impl UsageSource for Fake {
    fn processes(&self) -> Vec<Process> {
        self.listings.fetch_add(1, Ordering::SeqCst);
        process::parse_ps(self.table)
    }

    fn usage(&self, agent: Agent) -> Result<Usage, Failure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if let Some(answer) = self.script.lock().unwrap().pop_front() {
            return answer;
        }
        match agent {
            Agent::Claude => Ok(Usage {
                plan: "Max 5x".into(),
                windows: vec![Window {
                    label: "5h".into(),
                    used_percent: 10.0,
                    resets_at_ms: None,
                }],
            }),
            _ => Err("nope".into()),
        }
    }
}

fn monitor(table: &'static str) -> (UsageMonitor, Arc<Fake>) {
    let fake = Arc::new(Fake {
        table,
        reads: AtomicUsize::new(0),
        listings: AtomicUsize::new(0),
        script: Mutex::new(VecDeque::new()),
    });
    (UsageMonitor::new(fake.clone()), fake)
}

#[tokio::test]
async fn reports_the_agent_in_the_pane_and_caches_its_usage() {
    let (monitor, fake) = monitor(
        "10 1 10 0 -zsh\n11 10 11 0 claude\n20 1 20 0 -zsh\n30 1 30 0 -zsh\n31 30 31 0 cursor-agent\n",
    );
    let report = monitor.report(Some(10)).await;
    assert_eq!(report.agent, Some(Agent::Claude));
    assert_eq!(report.usage.as_ref().unwrap().plan, "Max 5x");
    // Again: both the process table and the usage come from the cache.
    monitor.report(Some(10)).await;
    assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
    assert_eq!(fake.listings.load(Ordering::SeqCst), 1);

    let plain = monitor.report(Some(20)).await;
    assert_eq!(plain.agent, None);
    assert_eq!(plain.usage, Ok(Usage::default()));

    let cursor = monitor.report(Some(30)).await;
    assert_eq!(cursor.agent, Some(Agent::Cursor));
    assert_eq!(cursor.usage, Err("nope".into()));

    assert_eq!(monitor.report(None).await.agent, None);
}

#[tokio::test]
async fn a_rate_limited_service_is_left_alone_and_the_last_reading_stays() {
    let (monitor, fake) = monitor("10 1 10 0 -zsh\n11 10 11 0 claude\n");
    let first = monitor.report(Some(10)).await;
    assert_eq!(first.usage.as_ref().unwrap().plan, "Max 5x");

    // The next read is refused.
    fake.script
        .lock()
        .unwrap()
        .push_back(Err(Failure::RateLimited {
            message: "Claude's usage service is busy".into(),
            retry_after: None,
        }));
    monitor.expire();
    let refused = monitor.report(Some(10)).await;
    assert_eq!(fake.reads.load(Ordering::SeqCst), 2);
    assert_eq!(
        refused.usage, first.usage,
        "the last good reading stays on show"
    );
    assert_eq!(refused.fetched_at, first.fetched_at);

    // Polling on doesn't ask again while the service wants to be left alone.
    for _ in 0..5 {
        monitor.report(Some(10)).await;
    }
    assert_eq!(fake.reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_rate_limit_before_any_reading_says_when_it_tries_again() {
    let (monitor, fake) = monitor("10 1 10 0 -zsh\n11 10 11 0 claude\n");
    fake.script
        .lock()
        .unwrap()
        .push_back(Err(Failure::RateLimited {
            message: "Claude's usage service is busy".into(),
            retry_after: Some(Duration::from_secs(120)),
        }));
    let report = monitor.report(Some(10)).await;
    assert_eq!(
        report.usage,
        Err("Claude's usage service is busy; trying again in 2m".into())
    );
}

#[test]
fn plan_labels() {
    assert_eq!(plan_label("max", "default_claude_max_20x"), "Max 20x");
    assert_eq!(plan_label("pro", "default_claude_pro"), "Pro");
    assert_eq!(plan_label("", ""), "");
    assert_eq!(plan_label("team", "tier_x"), "Team");
}
