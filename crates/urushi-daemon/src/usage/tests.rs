#![allow(clippy::unwrap_used, reason = "test code")]

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A fixed process table and usage, counting how often each is read.
struct Fake {
    table: &'static str,
    reads: AtomicUsize,
    listings: AtomicUsize,
}

impl UsageSource for Fake {
    fn processes(&self) -> Vec<Process> {
        self.listings.fetch_add(1, Ordering::SeqCst);
        process::parse_ps(self.table)
    }

    fn usage(&self, agent: Agent) -> Result<Usage, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
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

#[test]
fn plan_labels() {
    assert_eq!(plan_label("max", "default_claude_max_20x"), "Max 20x");
    assert_eq!(plan_label("pro", "default_claude_pro"), "Pro");
    assert_eq!(plan_label("", ""), "");
    assert_eq!(plan_label("team", "tier_x"), "Team");
}
