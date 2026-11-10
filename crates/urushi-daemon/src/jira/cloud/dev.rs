//! [`CloudJira`]'s read of an issue's pull requests from Jira's development info (ADR
//! 0036): the summary says which code hosts have any, then each GitHub host's detail
//! lists them. Most issues have none, so most reads are the one summary request.

use urushi_core::board::PullRequest;

use super::CloudJira;
use crate::http::encode_query;
use crate::jira::{Account, JiraError, JiraResult, parse};

impl CloudJira {
    pub(super) fn read_pull_requests(
        &self,
        account: &Account,
        issue_id: &str,
    ) -> JiraResult<Vec<PullRequest>> {
        let id = encode_query(issue_id);
        let summary = self.get(
            account,
            &format!("/rest/dev-status/latest/issue/summary?issueId={id}"),
        );
        let summary = match summary {
            Ok(summary) => summary,
            // No code host connected, or development info hidden from this account.
            Err(JiraError::NotFound(_) | JiraError::Forbidden(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut details = Vec::new();
        for kind in parse::dev::github_instance_types(&summary) {
            details.push(self.get(
                account,
                &format!(
                    "/rest/dev-status/latest/issue/detail?issueId={id}&applicationType={}\
                     &dataType=pullrequest",
                    encode_query(&kind)
                ),
            )?);
        }
        Ok(parse::dev::pull_requests(&details))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::tests::{Script, account};
    use super::*;
    use crate::jira::JiraApi;

    #[test]
    fn an_issue_without_pull_requests_takes_one_request() {
        let none = json!({ "summary": { "pullrequest": { "overall": { "count": 0 } } } });
        let http = Script::answering([(200, none)]);
        let jira = CloudJira::new(http.clone());
        assert!(jira.pull_requests(&account(), "10001").unwrap().is_empty());
        let seen = http.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].url,
            "https://acme.atlassian.net/rest/dev-status/latest/issue/summary?issueId=10001"
        );
    }

    #[test]
    fn github_pull_requests_are_read_from_the_detail() {
        let summary = json!({ "summary": { "pullrequest": {
            "overall": { "count": 1 },
            "byInstanceType": { "GitHub": { "count": 1, "name": "GitHub" } },
        } } });
        let detail = json!({ "detail": [{ "pullRequests": [{
            "name": "ACME-1 fix", "status": "OPEN",
            "url": "https://github.com/acme/app/pull/7",
            "lastUpdate": "2026-11-03T10:00:00.000+0000",
        }] }] });
        let http = Script::answering([(200, summary), (200, detail)]);
        let jira = CloudJira::new(http.clone());
        let prs = jira.pull_requests(&account(), "10001").unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].url.as_str(), "https://github.com/acme/app/pull/7");
        assert_eq!(
            http.seen()[1].url,
            "https://acme.atlassian.net/rest/dev-status/latest/issue/detail?issueId=10001\
             &applicationType=GitHub&dataType=pullrequest"
        );
    }

    #[test]
    fn a_site_without_development_info_has_no_pull_requests() {
        for status in [403, 404] {
            let http = Script::answering([(status, json!({ "errorMessages": ["no"] }))]);
            let jira = CloudJira::new(http);
            assert!(jira.pull_requests(&account(), "1").unwrap().is_empty());
        }
        let http = Script::answering([(401, json!({}))]);
        let jira = CloudJira::new(http);
        assert_eq!(
            jira.pull_requests(&account(), "1"),
            Err(JiraError::Unauthorized),
            "a bad sign-in still says so"
        );
    }
}
