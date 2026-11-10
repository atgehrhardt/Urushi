//! Reading Jira's development info (ADR 0036): which code hosts report pull requests
//! for an issue (`/rest/dev-status/latest/issue/summary`), and those pull requests
//! (`…/issue/detail?dataType=pullrequest`). These endpoints are what Jira's own
//! Development panel reads; they aren't in Jira's published API, so anything missing
//! or odd reads as "no pull requests" rather than failing.

use serde_json::Value;
use urushi_core::board::{MAX_PULL_REQUESTS, PrStatus, PrUrl, PullRequest};

/// The code-host instance types (as the summary names them) that report pull requests
/// for the issue: GitHub's only, since a card shows them with GitHub's mark.
pub(in crate::jira) fn github_instance_types(summary: &Value) -> Vec<String> {
    let prs = &summary["summary"]["pullrequest"];
    if prs["overall"]["count"].as_u64().unwrap_or(0) == 0 {
        return Vec::new();
    }
    prs["byInstanceType"]
        .as_object()
        .map(|types| {
            types
                .iter()
                .filter(|(_, t)| t["count"].as_u64().unwrap_or(0) > 0)
                .filter(|(kind, t)| {
                    let name = t["name"].as_str().unwrap_or_default();
                    is_github(kind) || is_github(name)
                })
                .map(|(kind, _)| kind.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn is_github(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("github")
}

/// The pull requests in detail answers: the most recently updated first, each once,
/// those without an `https://` address left out, at most [`MAX_PULL_REQUESTS`].
pub(in crate::jira) fn pull_requests<'a>(
    details: impl IntoIterator<Item = &'a Value>,
) -> Vec<PullRequest> {
    let mut found: Vec<(String, PullRequest)> = details
        .into_iter()
        .flat_map(|d| d["detail"].as_array().into_iter().flatten())
        .flat_map(|d| d["pullRequests"].as_array().into_iter().flatten())
        .filter_map(|p| {
            let url = PrUrl::parse(p["url"].as_str()?).ok()?;
            let pr = PullRequest::new(
                url,
                p["name"].as_str().unwrap_or_default(),
                PrStatus::from_jira(p["status"].as_str().unwrap_or_default()),
            );
            Some((p["lastUpdate"].as_str().unwrap_or_default().to_owned(), pr))
        })
        .collect();
    // ISO 8601 times in one zone sort as text.
    found.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out: Vec<PullRequest> = Vec::new();
    for (_, pr) in found {
        if !out.iter().any(|p| p.url == pr.url) {
            out.push(pr);
        }
    }
    out.truncate(MAX_PULL_REQUESTS);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn summary(count: u64, types: &Value) -> Value {
        json!({ "errors": [], "summary": { "pullrequest": {
            "overall": { "count": count, "state": "OPEN" },
            "byInstanceType": types,
        }, "branch": { "overall": { "count": 1 } } } })
    }

    #[test]
    fn only_github_hosts_with_pull_requests_are_read() {
        let s = summary(
            3,
            &json!({
                "GitHub": { "count": 1, "name": "GitHub" },
                "githube": { "count": 1, "name": "GitHub Enterprise" },
                "gitlab": { "count": 1, "name": "GitLab" },
                "bitbucket": { "count": 0, "name": "Bitbucket" },
            }),
        );
        let mut types = github_instance_types(&s);
        types.sort();
        assert_eq!(types, ["GitHub", "githube"]);
        assert!(github_instance_types(&summary(0, &json!({}))).is_empty());
        assert!(github_instance_types(&json!({})).is_empty());
        assert!(github_instance_types(&Value::Null).is_empty());
    }

    #[test]
    fn pull_requests_are_read_newest_first_once_each() {
        let pr = |n: u32, status: &str, updated: &str| {
            json!({
                "id": format!("#{n}"),
                "name": format!("ACME-1 change {n}"),
                "status": status,
                "url": format!("https://github.com/acme/app/pull/{n}"),
                "lastUpdate": updated,
            })
        };
        let one = json!({ "errors": [], "detail": [{
            "branches": [],
            "pullRequests": [
                pr(1, "MERGED", "2026-11-01T10:00:00.000+0000"),
                pr(2, "OPEN", "2026-11-03T10:00:00.000+0000"),
                { "name": "no address", "status": "OPEN" },
                { "name": "insecure", "url": "http://x/pull/3", "status": "OPEN" },
            ],
        }] });
        let two = json!({ "detail": [{ "pullRequests": [
            pr(2, "OPEN", "2026-11-03T10:00:00.000+0000"),
            pr(4, "DECLINED", "2026-11-02T10:00:00.000+0000"),
        ] }] });
        let prs = pull_requests([&one, &two]);
        let numbers: Vec<_> = prs.iter().map(|p| p.url.label()).collect();
        assert_eq!(numbers, ["#2", "#4", "#1"]);
        assert_eq!(prs[0].status, PrStatus::Open);
        assert_eq!(prs[0].title, "ACME-1 change 2");
        assert_eq!(prs[1].status, PrStatus::Declined);
        assert!(pull_requests([&json!({ "detail": "odd" })]).is_empty());
    }
}
