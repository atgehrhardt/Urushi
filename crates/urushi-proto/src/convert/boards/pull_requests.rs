//! Pull request links (ADR 0033) ↔ wire. A URL that isn't a valid [`PrUrl`] reads as
//! none rather than failing the whole board, so one bad link can't hide a board.

use urushi_core::board::{PrStatus, PrUrl, PullRequest};

use crate::v1;

/// A task's pull request link → wire (empty for none).
pub(super) fn pr_url_to_proto(url: Option<&PrUrl>) -> String {
    url.map(|u| u.as_str().to_owned()).unwrap_or_default()
}

/// Wire → a task's pull request link; empty or invalid reads as none.
pub(super) fn pr_url_from_proto(url: &str) -> Option<PrUrl> {
    PrUrl::parse(url).ok()
}

fn status_to_proto(s: PrStatus) -> i32 {
    let s = match s {
        PrStatus::Open => v1::PullRequestStatus::Open,
        PrStatus::Merged => v1::PullRequestStatus::Merged,
        PrStatus::Declined => v1::PullRequestStatus::Declined,
        PrStatus::Unknown => v1::PullRequestStatus::Unspecified,
    };
    s as i32
}

fn status_from_proto(s: i32) -> PrStatus {
    match v1::PullRequestStatus::try_from(s) {
        Ok(v1::PullRequestStatus::Open) => PrStatus::Open,
        Ok(v1::PullRequestStatus::Merged) => PrStatus::Merged,
        Ok(v1::PullRequestStatus::Declined) => PrStatus::Declined,
        _ => PrStatus::Unknown,
    }
}

/// Core pull request → wire.
pub(super) fn pull_request_to_proto(p: &PullRequest) -> v1::PullRequest {
    v1::PullRequest {
        url: p.url.as_str().to_owned(),
        title: p.title.clone(),
        status: status_to_proto(p.status),
    }
}

/// Wire pull requests → core; those with an invalid URL are left out.
pub(super) fn pull_requests_from_proto(prs: &[v1::PullRequest]) -> Vec<PullRequest> {
    prs.iter()
        .filter_map(|p| {
            let url = PrUrl::parse(&p.url).ok()?;
            Some(PullRequest::new(url, &p.title, status_from_proto(p.status)))
        })
        .collect()
}
