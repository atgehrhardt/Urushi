//! Agent usage reports → the wire (`AgentUsage`, protocol 1.5).

use urushi_proto::convert;
use urushi_proto::v1;

use crate::usage::Report;

/// A report → `AgentUsage`. No agent leaves everything empty.
pub(super) fn to_proto(report: &Report) -> v1::AgentUsage {
    let fetched_at_ms = i64::try_from(report.fetched_at.unix_millis()).unwrap_or(i64::MAX);
    let Some(agent) = report.agent else {
        return v1::AgentUsage {
            fetched_at_ms,
            ..Default::default()
        };
    };
    let (plan, windows, unavailable) = match &report.usage {
        Ok(usage) => (
            usage.plan.clone(),
            usage
                .windows
                .iter()
                .map(|w| v1::UsageWindow {
                    label: w.label.clone(),
                    used_percent: w.used_percent.clamp(0.0, 100.0),
                    resets_at_ms: w.resets_at_ms.unwrap_or(0),
                })
                .collect(),
            String::new(),
        ),
        Err(reason) => (String::new(), Vec::new(), reason.clone()),
    };
    v1::AgentUsage {
        agent: convert::agent_to_proto(agent).into(),
        plan,
        windows,
        unavailable,
        fetched_at_ms,
    }
}
