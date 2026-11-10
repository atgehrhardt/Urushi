//! Pull requests a task points at (ADR 0036).
//!
//! A local board's task can name one pull request by its URL ([`super::Task::pr_url`]);
//! a Jira issue's pull requests come from Jira's development info
//! ([`super::Issue::pull_requests`]). Either way a client shows them on the thread the
//! task is linked to and opens them in a browser, so a URL is only ever an `https://`
//! one ([`PrUrl`]).

use serde::{Deserialize, Serialize};

use super::{Board, BoardError};

/// Longest pull request URL, in characters.
pub const MAX_PR_URL: usize = 2048;
/// Most pull requests kept for one Jira issue.
pub const MAX_PULL_REQUESTS: usize = 10;
/// Longest pull request title kept, in characters.
pub const MAX_PR_TITLE: usize = 300;

/// A pull request's address: an `https://` URL with a host and no whitespace, which a
/// client can hand to the system browser as is.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PrUrl(String);

impl PrUrl {
    /// `url` (trimmed) if it is an `https://` URL with a host, no whitespace or control
    /// characters, and at most [`MAX_PR_URL`] characters.
    pub fn parse(url: &str) -> Result<Self, BoardError> {
        let url = url.trim();
        if url.chars().count() > MAX_PR_URL {
            return Err(BoardError::TooLong {
                what: "a pull request URL",
                max: MAX_PR_URL,
            });
        }
        let host = url
            .strip_prefix("https://")
            .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(""));
        let valid = host.is_some_and(|h| !h.is_empty())
            && !url.chars().any(|c| c.is_whitespace() || c.is_control());
        if valid {
            Ok(Self(url.to_owned()))
        } else {
            Err(BoardError::BadPrUrl)
        }
    }

    /// The URL.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The pull request's number when the URL ends in one, as GitHub's
    /// (`…/pull/12`), GitLab's (`…/merge_requests/12`) and Bitbucket's
    /// (`…/pull-requests/12`) do.
    pub fn number(&self) -> Option<u64> {
        let path = self.0.split(['?', '#']).next().unwrap_or("");
        let mut parts = path.trim_end_matches('/').rsplit('/');
        let number = parts.next()?.parse().ok()?;
        matches!(
            parts.next()?,
            "pull" | "pulls" | "merge_requests" | "pull-requests"
        )
        .then_some(number)
    }

    /// How to name it in a sentence: `#12`, or the URL when it has no number.
    pub fn label(&self) -> String {
        self.number()
            .map_or_else(|| self.0.clone(), |n| format!("#{n}"))
    }
}

impl TryFrom<String> for PrUrl {
    type Error = BoardError;

    fn try_from(url: String) -> Result<Self, Self::Error> {
        Self::parse(&url)
    }
}

impl From<PrUrl> for String {
    fn from(url: PrUrl) -> Self {
        url.0
    }
}

impl std::fmt::Display for PrUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a pull request stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrStatus {
    /// Open for review.
    Open,
    /// Merged.
    Merged,
    /// Closed without merging.
    Declined,
    /// A status this version doesn't know.
    #[default]
    Unknown,
}

impl PrStatus {
    /// The status Jira's development info names (`OPEN`, `MERGED`, `DECLINED`).
    pub fn from_jira(status: &str) -> Self {
        match status.trim().to_ascii_uppercase().as_str() {
            "OPEN" | "DRAFT" => Self::Open,
            "MERGED" => Self::Merged,
            "DECLINED" | "CLOSED" => Self::Declined,
            _ => Self::Unknown,
        }
    }

    /// The lowercase name (`"open"`), as shown and printed.
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Declined => "declined",
            Self::Unknown => "unknown",
        }
    }
}

/// A pull request linked to a Jira issue, as Jira's code-host integration reports it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PullRequest {
    /// Where it is.
    pub url: PrUrl,
    /// Its title (at most [`MAX_PR_TITLE`] characters).
    #[serde(default)]
    pub title: String,
    /// Where it stands.
    #[serde(default)]
    pub status: PrStatus,
}

impl PullRequest {
    /// A pull request with its title cut to one line of at most [`MAX_PR_TITLE`]
    /// characters.
    pub fn new(url: PrUrl, title: &str, status: PrStatus) -> Self {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        Self {
            url,
            title: title.chars().take(MAX_PR_TITLE).collect(),
            status,
        }
    }

    /// The one to show for several: the first open one, else the first.
    pub fn best(prs: &[Self]) -> Option<&Self> {
        prs.iter()
            .find(|p| p.status == PrStatus::Open)
            .or_else(|| prs.first())
    }
}

impl Board {
    /// The Jira ids of the issues whose task (at any depth) is linked to a thread: the
    /// ones whose pull requests are read (ADR 0036).
    pub fn linked_issue_ids(&self) -> Vec<String> {
        self.all_tasks()
            .filter(|t| t.thread.is_some())
            .filter_map(|t| t.issue.as_ref().map(|i| i.id.clone()))
            .collect()
    }

    /// Sets the pull requests of the issue with this Jira id (the first
    /// [`MAX_PULL_REQUESTS`]). Returns whether anything changed; an issue no longer on
    /// the board changes nothing.
    pub fn set_pull_requests(&mut self, issue_id: &str, mut prs: Vec<PullRequest>) -> bool {
        prs.truncate(MAX_PULL_REQUESTS);
        let Some(id) = self
            .all_tasks()
            .find(|t| t.issue.as_ref().is_some_and(|i| i.id == issue_id))
            .map(|t| t.id)
        else {
            return false;
        };
        let Some(issue) = self.task_mut(id).ok().and_then(|t| t.issue.as_mut()) else {
            return false;
        };
        if issue.pull_requests == prs {
            return false;
        }
        issue.pull_requests = prs;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_urls_with_a_host_are_pull_request_urls() {
        let ok = PrUrl::parse("  https://github.com/o/r/pull/12 ").unwrap();
        assert_eq!(ok.as_str(), "https://github.com/o/r/pull/12");
        for bad in [
            "",
            "http://github.com/o/r/pull/1",
            "https://",
            "https:///path",
            "github.com/o/r/pull/1",
            "https://github.com/o r/pull/1",
            "javascript:alert(1)",
            "https://x\u{7}y",
        ] {
            assert_eq!(PrUrl::parse(bad), Err(BoardError::BadPrUrl), "{bad:?}");
        }
        let long = format!("https://h/{}", "a".repeat(MAX_PR_URL));
        assert!(matches!(
            PrUrl::parse(&long),
            Err(BoardError::TooLong { .. })
        ));
    }

    #[test]
    fn a_url_ending_in_a_pull_request_number_is_named_by_it() {
        let label = |u: &str| PrUrl::parse(u).unwrap().label();
        assert_eq!(label("https://github.com/o/r/pull/12"), "#12");
        assert_eq!(
            label("https://github.com/o/r/pull/12/files"),
            "https://github.com/o/r/pull/12/files"
        );
        assert_eq!(label("https://gitlab.com/g/p/-/merge_requests/7/"), "#7");
        assert_eq!(label("https://bitbucket.org/w/r/pull-requests/3?x=1"), "#3");
        assert_eq!(label("https://example.com/12"), "https://example.com/12");
    }

    #[test]
    fn an_invalid_stored_url_is_refused_when_read() {
        let json = r#"{"url":"http://x/pull/1","title":"t","status":"open"}"#;
        assert!(serde_json::from_str::<PullRequest>(json).is_err());
        let json = r#"{"url":"https://x/pull/1"}"#;
        let pr: PullRequest = serde_json::from_str(json).unwrap();
        assert_eq!(pr.status, PrStatus::Unknown);
    }

    #[test]
    fn the_first_open_pull_request_is_the_one_shown() {
        let pr = |n: u32, status| {
            PullRequest::new(
                PrUrl::parse(&format!("https://h/pull/{n}")).unwrap(),
                "",
                status,
            )
        };
        assert_eq!(PullRequest::best(&[]), None);
        let prs = [
            pr(1, PrStatus::Merged),
            pr(2, PrStatus::Open),
            pr(3, PrStatus::Open),
        ];
        assert_eq!(PullRequest::best(&prs), Some(&prs[1]));
        let prs = [pr(1, PrStatus::Merged), pr(2, PrStatus::Declined)];
        assert_eq!(PullRequest::best(&prs), Some(&prs[0]));
    }

    #[test]
    fn jira_statuses_map_onto_the_known_ones() {
        assert_eq!(PrStatus::from_jira("OPEN"), PrStatus::Open);
        assert_eq!(PrStatus::from_jira("merged"), PrStatus::Merged);
        assert_eq!(PrStatus::from_jira("DECLINED"), PrStatus::Declined);
        assert_eq!(PrStatus::from_jira("STALE"), PrStatus::Unknown);
    }

    #[test]
    fn titles_are_kept_to_one_short_line() {
        let url = PrUrl::parse("https://h/pull/1").unwrap();
        let pr = PullRequest::new(url, "  Fix\n the   bug ", PrStatus::Open);
        assert_eq!(pr.title, "Fix the bug");
    }
}
