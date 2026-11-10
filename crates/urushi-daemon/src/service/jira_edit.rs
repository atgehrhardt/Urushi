//! Changing one Jira issue from the task editor (protocol 1.13): any field it lets the
//! account edit, its status, comments, attachments, links, watching and work log.
//!
//! As with every Jira change (ADR 0020), it is made in Jira first, under the board's
//! lock; then the issue is read back and merged into the board (the card shows the new
//! assignee, priority… at once), and the editor gets the issue as it is now. Jira
//! refusing changes nothing here.

use std::collections::HashMap;

use serde_json::Value;
use urushi_core::board::{MAX_ATTACHMENT_BYTES, file_name};
use urushi_core::{TaskId, Timestamp};

use super::jira_details::IssueAt;
use super::{Service, ServiceError, ServiceResult, blocking, lock};
use crate::jira::details::{Field, FieldKind, FieldValue, IssueDetails};
use crate::jira::{IssueEdit, IssueFields};

/// One change to an issue.
#[derive(Clone, Debug, PartialEq)]
pub enum IssueChange {
    /// New values for fields (by id).
    Edit(Vec<(String, FieldValue)>),
    /// Take a transition (by id).
    Transition(String),
    /// Add a comment (Markdown).
    AddComment(String),
    /// Change a comment.
    UpdateComment {
        /// Its id.
        id: String,
        /// The new text (Markdown).
        body: String,
    },
    /// Delete a comment (by id).
    DeleteComment(String),
    /// Attach a file.
    Attach {
        /// Its name.
        name: String,
        /// Its contents.
        data: Vec<u8>,
    },
    /// Delete an attachment (by id).
    DeleteAttachment(String),
    /// Link to another issue.
    Link {
        /// The kind of link, by name.
        kind: String,
        /// The other issue's key.
        key: String,
        /// This issue is on the outward side ("this blocks that").
        outward: bool,
    },
    /// Remove a link (by id).
    Unlink(String),
    /// Start or stop watching.
    Watch(bool),
    /// Log work.
    LogWork {
        /// How long.
        seconds: u64,
        /// When it started.
        started: Timestamp,
        /// What was done (Markdown).
        comment: String,
    },
}

/// Edits as Jira's API takes them, by the kinds `meta` gives the fields. Fields that
/// aren't there, or that Urushi can't write, are refused.
pub(super) fn written(
    meta: &[Field],
    edits: &[(String, FieldValue)],
) -> ServiceResult<Vec<(String, Value)>> {
    let kinds: HashMap<&str, FieldKind> = meta.iter().map(|f| (f.id.as_str(), f.kind)).collect();
    written_by(&kinds, edits)
}

fn written_by(
    kinds: &HashMap<&str, FieldKind>,
    edits: &[(String, FieldValue)],
) -> ServiceResult<Vec<(String, Value)>> {
    edits
        .iter()
        .map(|(id, value)| {
            let kind = kinds.get(id.as_str()).ok_or_else(|| {
                ServiceError::Invalid(format!("Jira doesn't let {id} be changed here"))
            })?;
            let json = value.to_json(*kind).map_err(ServiceError::Invalid)?;
            Ok((id.clone(), json))
        })
        .collect()
}

/// A time as Jira's work log takes it: `2026-11-09T10:00:00.000+0000`.
fn jira_time(t: Timestamp) -> String {
    crate::time::jira_time(i64::try_from(t.unix_millis()).unwrap_or(i64::MAX))
}

impl Service {
    /// Makes `change` to task `id`'s issue in Jira, and answers with the issue as it
    /// is afterwards.
    pub async fn change_jira_issue(
        &self,
        id: TaskId,
        change: IssueChange,
    ) -> ServiceResult<IssueDetails> {
        let at = self.issue_at(id)?;
        let mut change = change;
        if let IssueChange::Attach { name, data } = &mut change {
            // Only the last part of a path, cleaned, is a file's name.
            *name = file_name(name)?;
            if data.len() > MAX_ATTACHMENT_BYTES {
                return Err(ServiceError::Invalid(format!(
                    "{name} is bigger than {} MiB",
                    MAX_ATTACHMENT_BYTES / (1024 * 1024)
                )));
            }
        }
        // Edits need the fields' kinds: from when the issue was opened, or read now.
        let kinds = if matches!(change, IssueChange::Edit(_)) {
            let known = lock(&self.jira.kinds).get(&at.key).cloned();
            if let Some(kinds) = known {
                kinds
            } else {
                self.read_issue_details(&at).await?;
                let read = lock(&self.jira.kinds).get(&at.key).cloned();
                read.unwrap_or_default()
            }
        } else {
            HashMap::new()
        };
        let edit = match &change {
            IssueChange::Edit(edits) => {
                let kinds: HashMap<&str, FieldKind> =
                    kinds.iter().map(|(k, v)| (k.as_str(), *v)).collect();
                Some(written_by(&kinds, edits)?)
            }
            _ => None,
        };
        self.write_change(&at, change, edit).await?;
        self.read_issue_details(&at).await
    }

    async fn write_change(
        &self,
        at: &IssueAt,
        change: IssueChange,
        edit: Option<Vec<(String, Value)>>,
    ) -> ServiceResult<()> {
        let account = self.jira_account()?;
        let guard = self.jira.lock(at.board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let (key, fields) = (at.key.clone(), IssueFields::of(&at.jira));
        let read = blocking(move || {
            let a = &account;
            let done = match change {
                IssueChange::Edit(_) => api.edit(
                    a,
                    &key,
                    &IssueEdit {
                        fields: edit.unwrap_or_default(),
                        ..IssueEdit::default()
                    },
                ),
                IssueChange::Transition(t) => api.transition(a, &key, &t),
                IssueChange::AddComment(body) => api.add_comment(a, &key, &body),
                IssueChange::UpdateComment { id, body } => api.update_comment(a, &key, &id, &body),
                IssueChange::DeleteComment(id) => api.delete_comment(a, &key, &id),
                IssueChange::Attach { name, data } => api.attach(a, &key, &name, &data),
                IssueChange::DeleteAttachment(id) => api.delete_attachment(a, &id),
                IssueChange::Link {
                    kind,
                    key: other,
                    outward,
                } => {
                    let (from, to) = if outward {
                        (&key, &other)
                    } else {
                        (&other, &key)
                    };
                    api.link(a, &kind, from, to)
                }
                IssueChange::Unlink(id) => api.unlink(a, &id),
                IssueChange::Watch(watch) => api.watch(a, &key, watch),
                IssueChange::LogWork {
                    seconds,
                    started,
                    comment,
                } => api.log_work(a, &key, seconds, &jira_time(started), &comment),
            };
            done?;
            Ok(api.issue(a, &key, &fields)?)
        })
        .await?;
        self.merge_written(at.board, vec![read])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_are_written_by_their_fields_kinds() {
        let field = |id: &str, kind| Field {
            id: id.into(),
            name: id.into(),
            kind,
            required: false,
            allowed: Vec::new(),
            value: FieldValue::default(),
        };
        let meta = [
            field("duedate", FieldKind::Date),
            field("x", FieldKind::Other),
        ];
        let date = FieldValue {
            text: "2026-12-01".into(),
            options: Vec::new(),
        };
        assert_eq!(
            written(&meta, &[("duedate".into(), date.clone())]).unwrap(),
            [("duedate".to_owned(), serde_json::json!("2026-12-01"))]
        );
        assert!(written(&meta, &[("x".into(), date.clone())]).is_err());
        assert!(written(&meta, &[("nope".into(), date)]).is_err());
        assert_eq!(
            jira_time(Timestamp::from_unix_millis(1_794_219_630_123)),
            "2026-11-09T10:20:30.123+0000"
        );
    }
}
