//! Typed wrappers for one Jira issue in full (protocol 1.13): what the editor shows,
//! changing it, its attachments, and the people and issues pickers search for.

use urushi_proto::v1::{self, change_jira_issue_request::Change, request::Body, response::Outcome};

use crate::requests::unexpected;
use crate::{Client, ClientError};

impl Client {
    /// Everything the editor shows of the issue a task shows.
    pub async fn jira_issue(&self, task_id: &str) -> Result<v1::JiraIssueDetails, ClientError> {
        let body = Body::GetJiraIssue(v1::GetJiraIssueRequest {
            task_id: task_id.into(),
        });
        self.details(body, "GetJiraIssue").await
    }

    /// Makes one change to a task's issue in Jira; the issue as it is afterwards.
    pub async fn change_jira_issue(
        &self,
        task_id: &str,
        change: Change,
    ) -> Result<v1::JiraIssueDetails, ClientError> {
        let body = Body::ChangeJiraIssue(v1::ChangeJiraIssueRequest {
            task_id: task_id.into(),
            change: Some(change),
        });
        self.details(body, "ChangeJiraIssue").await
    }

    /// The fields a new issue of `kind` on a Jira board has.
    pub async fn jira_create_fields(
        &self,
        board_id: &str,
        kind: v1::TaskKind,
    ) -> Result<Vec<v1::JiraField>, ClientError> {
        let body = Body::GetJiraCreateFields(v1::GetJiraCreateFieldsRequest {
            board_id: board_id.into(),
            kind: kind.into(),
        });
        Ok(self.details(body, "GetJiraCreateFields").await?.fields)
    }

    /// An attachment of a task's issue: the file, or an image's thumbnail.
    pub async fn jira_attachment(
        &self,
        task_id: &str,
        attachment_id: &str,
        thumbnail: bool,
    ) -> Result<v1::AttachmentData, ClientError> {
        let body = Body::GetJiraAttachment(v1::GetJiraAttachmentRequest {
            task_id: task_id.into(),
            attachment_id: attachment_id.into(),
            thumbnail,
        });
        match self.request(body).await? {
            Outcome::AttachmentData(data) => Ok(data),
            _ => Err(unexpected("GetJiraAttachment")),
        }
    }

    /// People matching `query` (who may be assigned the task's issue, if `assignable`),
    /// searched with the sign-in of the task's board, else of `board_id`'s (1.29).
    pub async fn search_jira_users(
        &self,
        (task_id, board_id): (&str, &str),
        query: &str,
        assignable: bool,
    ) -> Result<Vec<v1::JiraUser>, ClientError> {
        let body = Body::SearchJiraUsers(v1::SearchJiraUsersRequest {
            task_id: task_id.into(),
            query: query.into(),
            assignable,
            board_id: board_id.into(),
        });
        match self.request(body).await? {
            Outcome::JiraUsers(users) => Ok(users.users),
            _ => Err(unexpected("SearchJiraUsers")),
        }
    }

    /// Issues of a Jira board's project matching `query`.
    pub async fn search_jira_issues(
        &self,
        board_id: &str,
        query: &str,
    ) -> Result<Vec<v1::JiraIssueRef>, ClientError> {
        let body = Body::SearchJiraIssues(v1::SearchJiraIssuesRequest {
            board_id: board_id.into(),
            query: query.into(),
        });
        match self.request(body).await? {
            Outcome::JiraIssueRefs(refs) => Ok(refs.issues),
            _ => Err(unexpected("SearchJiraIssues")),
        }
    }

    async fn details(
        &self,
        body: Body,
        what: &'static str,
    ) -> Result<v1::JiraIssueDetails, ClientError> {
        match self.request(body).await? {
            Outcome::JiraIssueDetails(details) => Ok(details),
            _ => Err(unexpected(what)),
        }
    }
}
