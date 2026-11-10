//! Claude Code's plan usage: the same numbers its `/usage` screen shows.
//!
//! Claude Code keeps its sign-in in the OS credential store (the macOS keychain item
//! "Claude Code-credentials"; `~/.claude/.credentials.json` elsewhere). With that token,
//! `GET https://api.anthropic.com/api/oauth/usage` answers with the plan's windows. The
//! endpoint isn't a published API, so parsing is lenient and an unexpected answer is
//! reported as "unavailable" rather than an error. The request goes through `curl`, with
//! the token passed on stdin so it never shows in a process listing (ADR 0011).
//!
//! The endpoint is rate-limited per sign-in, a budget Claude Code itself shares. A 429
//! comes back as [`Failure::RateLimited`] with its `Retry-After`, so the monitor can wait
//! as long as asked instead of keeping the budget exhausted.

use std::time::Duration;

use serde_json::Value;

use super::{Failure, Usage, Window, plan_label};
use crate::exec;
use crate::time::rfc3339_ms;

const URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA: &str = "oauth-2025-04-20";
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the stored sign-in says.
#[derive(Debug, PartialEq)]
pub(crate) struct Credentials {
    pub(crate) token: String,
    pub(crate) expires_at_ms: Option<i64>,
    pub(crate) plan: String,
}

/// Reads the sign-in and asks for usage.
pub(crate) fn read() -> Result<Usage, Failure> {
    let json = crate::platform::claude_credentials()?;
    let credentials = parse_credentials(&json)?;
    if credentials.expires_at_ms.is_some_and(|at| at <= now_ms()) {
        return Err("Claude's sign-in has expired; it renews the next time Claude runs".into());
    }
    let headers = format!(
        "Authorization: Bearer {}\nanthropic-beta: {BETA}\nAccept: application/json\n",
        credentials.token
    );
    let out = exec::run(
        "curl",
        &[
            "--silent",
            "--show-error",
            "--max-time",
            "10",
            "--header",
            "@-",
            "--write-out",
            "\n%header{retry-after}\n%{http_code}",
            URL,
        ],
        Some(headers.as_bytes()),
        TIMEOUT,
    )?;
    answer(&String::from_utf8_lossy(&out), credentials.plan)
}

/// Usage from curl's output: the body, then the `Retry-After` header and the HTTP status
/// on a line each (see the `--write-out` in [`read`]).
pub(crate) fn answer(curl_output: &str, plan: String) -> Result<Usage, Failure> {
    let mut lines = curl_output.rsplitn(3, '\n');
    let status = lines.next().unwrap_or_default().trim();
    let retry_after = lines.next().unwrap_or_default().trim();
    let body = lines.next().unwrap_or_default();
    match status {
        "200" => Ok(Usage {
            plan,
            windows: parse_usage(body)?,
        }),
        "401" | "403" => {
            Err("Claude didn't accept its stored sign-in; run /login in Claude".into())
        }
        "429" => Err(Failure::RateLimited {
            message: "Claude's usage service is busy".into(),
            // Seconds; the HTTP-date form is rare and falls back to the default wait.
            retry_after: retry_after.parse().ok().map(Duration::from_secs),
        }),
        other => Err(format!("Claude's usage service answered HTTP {other}").into()),
    }
}

fn now_ms() -> i64 {
    i64::try_from(crate::service::now().unix_millis()).unwrap_or(i64::MAX)
}

/// The token, its expiry and the plan, from Claude Code's stored credentials.
pub(crate) fn parse_credentials(json: &str) -> Result<Credentials, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|_| "Claude's stored sign-in is unreadable")?;
    let oauth = value
        .get("claudeAiOauth")
        .ok_or("Claude is signed in with an API key, which has no plan limits")?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or("Claude isn't signed in")?;
    let text = |key: &str| oauth.get(key).and_then(Value::as_str).unwrap_or_default();
    Ok(Credentials {
        token: token.to_owned(),
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_i64),
        plan: plan_label(text("subscriptionType"), text("rateLimitTier")),
    })
}

/// The windows in a usage answer, shortest first.
///
/// Newer answers list `limits` (`group`, `percent`, `resets_at`, an optional model
/// `scope`, `is_active`); older ones have `five_hour`, `seven_day`, `seven_day_opus` and
/// `seven_day_sonnet` objects (`utilization`, `resets_at`). Both are understood.
pub(crate) fn parse_usage(json: &str) -> Result<Vec<Window>, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|_| "Claude's usage answer is unreadable")?;
    let mut windows: Vec<(u8, Window)> = value
        .get("limits")
        .and_then(Value::as_array)
        .map(|limits| limits.iter().filter_map(limit).collect())
        .unwrap_or_default();
    if windows.is_empty() {
        let fields = [
            ("five_hour", "5h", 0),
            ("seven_day", "Week", 1),
            ("seven_day_opus", "Week · Opus", 2),
            ("seven_day_sonnet", "Week · Sonnet", 2),
        ];
        windows = fields
            .iter()
            .filter_map(|(key, label, rank)| {
                let w = value.get(key).filter(|w| w.is_object())?;
                Some((
                    *rank,
                    Window {
                        label: (*label).to_owned(),
                        used_percent: w.get("utilization").and_then(Value::as_f64)?,
                        resets_at_ms: w
                            .get("resets_at")
                            .and_then(Value::as_str)
                            .and_then(rfc3339_ms),
                    },
                ))
            })
            .collect();
    }
    if windows.is_empty() {
        return Err("Claude reported no plan limits".into());
    }
    windows.sort_by_key(|(rank, _)| *rank);
    Ok(windows.into_iter().map(|(_, w)| w).collect())
}

/// One entry of `limits`, with its sort rank. Inactive limits at 0% are left out.
fn limit(l: &Value) -> Option<(u8, Window)> {
    let used = l.get("percent").and_then(Value::as_f64)?;
    let active = l.get("is_active").and_then(Value::as_bool).unwrap_or(true);
    if !active && used <= 0.0 {
        return None;
    }
    let group = l.get("group").and_then(Value::as_str).unwrap_or_default();
    let (base, rank) = match group {
        "session" | "five_hour" => ("5h".to_owned(), 0),
        "daily" => ("Day".to_owned(), 1),
        "weekly" => ("Week".to_owned(), 2),
        "monthly" => ("Month".to_owned(), 3),
        other => (super::capitalize(&other.replace('_', " ")), 4),
    };
    let scope = l
        .pointer("/scope/model/display_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    Some((
        rank + u8::from(scope.is_some()),
        Window {
            label: scope.map_or_else(|| base.clone(), |s| format!("{base} · {s}")),
            used_percent: used,
            resets_at_ms: l
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(rfc3339_ms),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_give_the_token_and_plan() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-x","expiresAt":1794080504725,
            "subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#;
        assert_eq!(
            parse_credentials(json).unwrap(),
            Credentials {
                token: "sk-x".into(),
                expires_at_ms: Some(1_794_080_504_725),
                plan: "Max 20x".into(),
            }
        );
        let pro = r#"{"claudeAiOauth":{"accessToken":"t","subscriptionType":"pro"}}"#;
        assert_eq!(parse_credentials(pro).unwrap().plan, "Pro");
        assert!(parse_credentials("{}").unwrap_err().contains("API key"));
        assert!(parse_credentials("not json").is_err());
    }

    #[test]
    fn limits_are_read_and_inactive_empty_ones_skipped() {
        let json = r#"{"five_hour":null,"seven_day":{"utilization":3.0},
          "limits":[
            {"kind":"weekly_all","group":"weekly","percent":3,"resets_at":"2026-11-13T05:59:59Z","scope":null,"is_active":true},
            {"kind":"weekly_scoped","group":"weekly","percent":0,"resets_at":"2026-11-13T06:00:00Z",
             "scope":{"model":{"display_name":"Fable"}},"is_active":false},
            {"kind":"session","group":"session","percent":41.5,"resets_at":"2026-11-07T20:00:00Z","is_active":true},
            {"kind":"weekly_scoped","group":"weekly","percent":12,"scope":{"model":{"display_name":"Opus"}},"is_active":true}
          ]}"#;
        let windows = parse_usage(json).unwrap();
        let labels: Vec<_> = windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["5h", "Week", "Week · Opus"]);
        assert!((windows[0].used_percent - 41.5).abs() < f64::EPSILON);
        assert_eq!(windows[1].resets_at_ms, rfc3339_ms("2026-11-13T05:59:59Z"));
        assert_eq!(windows[2].resets_at_ms, None);
    }

    #[test]
    fn answers_are_split_from_the_status_and_retry_after() {
        let ok = "{\"seven_day\":\n{\"utilization\":5}}\n\n200";
        let usage = answer(ok, "Pro".into()).unwrap();
        assert_eq!(usage.plan, "Pro");
        assert_eq!(usage.windows[0].label, "Week");

        let busy = "{\"error\":{\"type\":\"rate_limit_error\"}}\n120\n429";
        assert_eq!(
            answer(busy, String::new()),
            Err(Failure::RateLimited {
                message: "Claude's usage service is busy".into(),
                retry_after: Some(Duration::from_secs(120)),
            })
        );
        let dated = "{}\nWed, 21 Oct 2026 07:28:00 GMT\n429";
        assert!(matches!(
            answer(dated, String::new()),
            Err(Failure::RateLimited {
                retry_after: None,
                ..
            })
        ));
        assert!(matches!(
            answer("{}\n\n401", String::new()),
            Err(Failure::Unavailable(m)) if m.contains("/login")
        ));
        assert_eq!(
            answer("<html>\n\n502", String::new()),
            Err("Claude's usage service answered HTTP 502".into())
        );
    }

    #[test]
    fn older_answers_without_limits_are_understood() {
        let json = r#"{"five_hour":{"utilization":20,"resets_at":"2026-11-07T20:00:00+00:00"},
          "seven_day":{"utilization":5.5,"resets_at":null},"seven_day_opus":null}"#;
        let windows = parse_usage(json).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[1].label, "Week");
        assert!(parse_usage(r#"{"limits":[]}"#).is_err());
        assert!(parse_usage("<html>").is_err());
    }
}
