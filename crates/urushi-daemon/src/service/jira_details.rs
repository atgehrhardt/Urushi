//! Reading one Jira issue in full for the task editor (protocol 1.13): its fields,
//! comments, attachments, links, watchers and work log; the fields a new issue has;
//! attachments' contents; and the people and issues pickers offer.
//!
//! None of this is kept on the board (the workspace is resent whole on every change):
//! it is read from Jira when asked. The kinds of an issue's fields are remembered so
//! edits can be written as Jira takes them (`jira_edit.rs`).

use std::collections::HashMap;

use urushi_core::board::{JiraBoard, MAX_ATTACHMENT_BYTES, Person};
use urushi_core::{BoardId, TaskId, TaskKind};

use super::{AttachmentData, Service, ServiceError, ServiceResult, blocking, lock};
use crate::jira::details::{Field, IssueDetails, IssueRef};

/// Most issues an issue search answers with.
const ISSUES_FOUND: usize = 20;

/// A Jira issue a task shows: its board's Jira side and its key.
pub(super) struct IssueAt {
    pub(super) board: BoardId,
    pub(super) jira: JiraBoard,
    pub(super) key: String,
}

impl Service {
    /// The Jira issue task `id` shows.
    pub(super) fn issue_at(&self, id: TaskId) -> ServiceResult<IssueAt> {
        self.read(|state| {
            let board = state
                .boards
                .iter()
                .find(|b| b.task(id).is_some())
                .ok_or_else(|| ServiceError::NotFound(format!("task {id}")))?;
            let jira = board.jira().cloned().ok_or_else(|| {
                ServiceError::Precondition("that task isn't on a Jira board".into())
            })?;
            let key = board
                .task(id)
                .and_then(|t| t.issue.as_ref())
                .map(|i| i.key.clone())
                .ok_or_else(|| ServiceError::Precondition("that task isn't a Jira issue".into()))?;
            Ok(IssueAt {
                board: board.id,
                jira,
                key,
            })
        })
    }

    /// Everything the editor shows of task `id`'s issue.
    pub async fn jira_issue_details(&self, id: TaskId) -> ServiceResult<IssueDetails> {
        let at = self.issue_at(id)?;
        self.read_issue_details(&at).await
    }

    pub(super) async fn read_issue_details(&self, at: &IssueAt) -> ServiceResult<IssueDetails> {
        let account = self.jira_account()?;
        let api = self.jira.api.clone();
        let (key, board) = (at.key.clone(), at.jira.board_id);
        let details = blocking(move || Ok(api.details(&account, &key, board)?)).await?;
        self.remember_kinds(&at.key, &details.fields);
        Ok(details)
    }

    /// Keeps the kinds of an issue's fields, for writing edits to them.
    fn remember_kinds(&self, key: &str, fields: &[Field]) {
        let kinds: HashMap<_, _> = fields.iter().map(|f| (f.id.clone(), f.kind)).collect();
        let mut all = lock(&self.jira.kinds);
        // Bounded: issues opened long ago are read again when edited.
        if all.len() >= 256 {
            all.clear();
        }
        all.insert(key.to_owned(), kinds);
    }

    /// The fields a new issue of `kind` on `board` has.
    pub async fn jira_create_fields(
        &self,
        board: BoardId,
        kind: TaskKind,
    ) -> ServiceResult<Vec<Field>> {
        let jira = self.read(|state| state.jira_board(board))?;
        let account = self.jira_account()?;
        let api = self.jira.api.clone();
        blocking(move || Ok(api.create_fields(&account, &jira.project_key, kind)?)).await
    }

    /// People matching `query`: who may be assigned task `id`'s issue, or anyone
    /// (without a task: a new issue).
    pub async fn search_jira_users(
        &self,
        id: Option<TaskId>,
        query: &str,
        assignable: bool,
    ) -> ServiceResult<Vec<Person>> {
        let key = match id {
            Some(id) if assignable => Some(self.issue_at(id)?.key),
            _ => None,
        };
        let account = self.jira_account()?;
        let api = self.jira.api.clone();
        let query = query.to_owned();
        blocking(move || Ok(api.users(&account, &query, key.as_deref())?)).await
    }

    /// Issues of `board`'s project matching `query`, recently updated first.
    pub async fn search_jira_issues(
        &self,
        board: BoardId,
        query: &str,
    ) -> ServiceResult<Vec<IssueRef>> {
        let jira = self.read(|state| state.jira_board(board))?;
        let account = self.jira_account()?;
        let api = self.jira.api.clone();
        let query = query.to_owned();
        blocking(move || Ok(api.search(&account, &jira.project_key, &query, ISSUES_FOUND)?)).await
    }

    /// An attachment of task `id`'s issue: the file, or an image's thumbnail.
    pub async fn jira_attachment(
        &self,
        id: TaskId,
        attachment: &str,
        thumbnail: bool,
    ) -> ServiceResult<AttachmentData> {
        let details = self.jira_issue_details(id).await?;
        let file = details
            .attachments
            .into_iter()
            .find(|a| a.id == attachment)
            .ok_or_else(|| ServiceError::NotFound(format!("attachment {attachment}")))?;
        // Files travel in one frame; images are only decoded up to a bound.
        let limit = if thumbnail {
            MAX_ATTACHMENT_BYTES * 4
        } else {
            MAX_ATTACHMENT_BYTES
        };
        if usize::try_from(file.size).unwrap_or(usize::MAX) > limit {
            return Err(ServiceError::Invalid(format!(
                "{} is too big to open here; open it in Jira",
                file.name
            )));
        }
        let account = self.jira_account()?;
        let api = self.jira.api.clone();
        let url = file.content_url.clone();
        let bytes = blocking(move || Ok(api.download(&account, &url)?)).await?;
        if !thumbnail {
            return Ok(AttachmentData::File {
                media_type: file.media_type,
                bytes,
            });
        }
        let decoded = blocking(move || {
            crate::attachments::decode(&bytes)
                .map_err(|_| ServiceError::Precondition(format!("{} isn't an image", file.name)))
        })
        .await?;
        Ok(AttachmentData::Thumbnail {
            width: decoded.thumbnail.width,
            height: decoded.thumbnail.height,
            rgba: decoded.thumbnail.rgba,
        })
    }
}
