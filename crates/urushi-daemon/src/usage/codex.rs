//! Codex's plan usage, from its own session logs.
//!
//! Codex writes each session to `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`
//! (`~/.codex` by default). After every model response it logs a `token_count` event
//! carrying `rate_limits`: a `primary` and `secondary` window (`used_percent`,
//! `window_minutes`, and `resets_at` in epoch seconds, or `resets_in_seconds` in older
//! versions) and the `plan_type`. The newest such record is what `/status` shows.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Usage, Window, capitalize, time::rfc3339_ms};

/// How much of the end of a session file is searched.
const TAIL: u64 = 1024 * 1024;
/// How many recent session files are searched for a rate-limit record.
const FILES: usize = 12;

/// Codex's home: `$CODEX_HOME`, else `~/.codex`.
pub(crate) fn home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(home));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".codex"))
}

/// The newest rate limits Codex logged under `home`.
pub(crate) fn read(home: &Path) -> Result<Usage, String> {
    let now = i64::try_from(crate::service::now().unix_millis()).unwrap_or(i64::MAX);
    for file in recent_sessions(&home.join("sessions")) {
        if let Some(usage) = tail(&file).and_then(|text| latest(&text, now)) {
            return Ok(usage);
        }
    }
    Err("Codex hasn't reported its limits yet; they appear after its first reply".into())
}

/// Session files, newest first (their paths sort by date, their names by time).
fn recent_sessions(sessions: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(sessions.to_path_buf(), 0)];
    // Walk year → month → day, newest first, until enough files are found.
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        names.sort();
        if depth == 3 {
            let mut files: Vec<PathBuf> = names
                .into_iter()
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("rollout-"))
                        && p.extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
                })
                .collect();
            files.reverse();
            found.extend(files);
            if found.len() >= FILES {
                break;
            }
        } else {
            // Pushed oldest first, so the newest is popped first.
            stack.extend(
                names
                    .into_iter()
                    .filter(|p| p.is_dir())
                    .map(|p| (p, depth + 1)),
            );
        }
    }
    found.truncate(FILES);
    found
}

fn tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::new();
    file.take(TAIL).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The last rate-limit record in a session log.
pub(crate) fn latest(log: &str, now_ms: i64) -> Option<Usage> {
    log.lines()
        .rev()
        .filter(|line| line.contains("\"rate_limits\""))
        .find_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let limits = value
                .pointer("/payload/rate_limits")
                .or_else(|| value.pointer("/payload/info/rate_limits"))
                .or_else(|| value.get("rate_limits"))
                .filter(|l| l.is_object())?;
            let logged_at = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(rfc3339_ms);
            parse(limits, logged_at, now_ms)
        })
}

/// A `rate_limits` object → usage. A window that has reset since it was logged is
/// shown as unused.
pub(crate) fn parse(limits: &Value, logged_at_ms: Option<i64>, now_ms: i64) -> Option<Usage> {
    let windows: Vec<Window> = ["primary", "secondary"]
        .iter()
        .filter_map(|key| {
            let w = limits.get(key).filter(|w| w.is_object())?;
            let used = w.get("used_percent").and_then(Value::as_f64)?;
            let minutes = w.get("window_minutes").and_then(Value::as_i64);
            let resets_at_ms = w
                .get("resets_at")
                .and_then(Value::as_i64)
                .map(|s| s * 1000)
                .or_else(|| {
                    let secs = w.get("resets_in_seconds").and_then(Value::as_i64)?;
                    Some(logged_at_ms? + secs * 1000)
                });
            let reset_since = resets_at_ms.is_some_and(|at| at <= now_ms);
            Some(Window {
                label: minutes.map_or_else(|| (*key).to_owned(), window_label),
                used_percent: if reset_since { 0.0 } else { used },
                resets_at_ms: resets_at_ms.filter(|_| !reset_since),
            })
        })
        .collect();
    if windows.is_empty() {
        return None;
    }
    let mut windows = windows;
    windows.sort_by_key(|w| window_rank(&w.label));
    Some(Usage {
        plan: limits
            .get("plan_type")
            .and_then(Value::as_str)
            .map(capitalize)
            .unwrap_or_default(),
        windows,
    })
}

/// 300 → "5h", 10080 → "Week", 1440 → "Day".
fn window_label(minutes: i64) -> String {
    match minutes {
        10_080 => "Week".into(),
        1_440 => "Day".into(),
        m if m > 0 && m % 1_440 == 0 => format!("{}d", m / 1_440),
        m if m > 0 && m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

fn window_rank(label: &str) -> u8 {
    match label {
        l if l.ends_with('m') => 0,
        l if l.ends_with('h') => 1,
        "Day" => 2,
        l if l.ends_with('d') => 3,
        "Week" => 4,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"timestamp":"2026-09-30T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","primary":{"used_percent":12.0,"window_minutes":300,"resets_at":1791333065},"secondary":{"used_percent":40.0,"window_minutes":10080,"resets_in_seconds":3600},"plan_type":"pro"}}}"#;

    #[test]
    fn the_last_record_is_read_with_both_reset_styles() {
        let logged = rfc3339_ms("2026-09-30T00:00:00Z").unwrap();
        let log = format!("{{\"type\":\"other\"}}\n{LINE}\nnot json\n");
        let usage = latest(&log, logged).unwrap();
        assert_eq!(usage.plan, "Pro");
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "5h");
        assert_eq!(usage.windows[0].resets_at_ms, Some(1_791_333_065_000));
        assert_eq!(usage.windows[1].label, "Week");
        assert_eq!(usage.windows[1].resets_at_ms, Some(logged + 3_600_000));
        assert!((usage.windows[1].used_percent - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn windows_that_reset_since_count_as_unused() {
        let much_later = 1_791_333_065_000 + 1;
        let usage = latest(LINE, much_later).unwrap();
        assert!(usage.windows[0].used_percent.abs() < f64::EPSILON);
        assert_eq!(usage.windows[0].resets_at_ms, None);
    }

    #[test]
    fn labels_and_missing_records() {
        assert_eq!(window_label(300), "5h");
        assert_eq!(window_label(2_880), "2d");
        assert_eq!(window_label(45), "45m");
        assert!(latest("{\"rate_limits\":{\"primary\":null}}", 0).is_none());
        assert!(latest("", 0).is_none());
    }

    #[test]
    fn the_newest_session_with_limits_wins() {
        let home = tempfile::tempdir().unwrap();
        let day = home.path().join("sessions/2026/09/30");
        std::fs::create_dir_all(&day).unwrap();
        let older = home.path().join("sessions/2026/09/29");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(older.join("rollout-2026-09-29T10-00-00-a.jsonl"), "{}\n").unwrap();
        std::fs::write(
            day.join("rollout-2026-09-30T09-00-00-b.jsonl"),
            format!("{LINE}\n"),
        )
        .unwrap();
        // The newest session hasn't had a reply yet: fall back to the one before.
        std::fs::write(day.join("rollout-2026-09-30T10-00-00-c.jsonl"), "{}\n").unwrap();
        let usage = read(home.path()).unwrap();
        assert_eq!(usage.plan, "Pro");
        assert!(read(&home.path().join("missing")).is_err());
    }
}
