//! The task editor's Jira side (`issue.rs`, protocol 1.13): asking for the issue in
//! full when its editor opens, field drafts and pickers, and the changes made at once
//! (status, comments, attachments, links, watching, work), each answered with the
//! issue as it is afterwards.

use urushi_client::v1::{
    self, change_jira_issue_request::Change, request::Body, response::Outcome,
};
use urushi_ui::TextField;

use super::request;
use crate::issue::{ISSUES_SINCE_MINOR, IssueEditor, LINK, duration};
use crate::msg::{Effect, Followup, IssueMsg};
use crate::state::{AppState, Dialog, Field};

/// The editor of `task`, if it is the one on screen or one it was opened from.
fn editor_of<'a>(dialog: &'a mut Option<Dialog>, task: &str) -> Option<&'a mut IssueEditor> {
    let Some(Dialog::Task {
        task: current,
        issue,
        nav,
        ..
    }) = dialog
    else {
        return None;
    };
    if current.as_deref() == Some(task) {
        return Some(issue);
    }
    nav.back.iter_mut().find_map(|d| match d {
        Dialog::Task {
            task: Some(t),
            issue,
            ..
        } if t == task => Some(issue.as_mut()),
        _ => None,
    })
}

impl AppState {
    /// Whether the daemon shows Jira issues in full.
    pub(crate) fn daemon_supports_jira_issues(&self) -> bool {
        self.daemon_minor >= ISSUES_SINCE_MINOR
    }

    /// The task on screen in the editor (empty for a new one) and its Jira side.
    fn issue_editor(&mut self) -> Option<(String, &mut IssueEditor)> {
        let Some(Dialog::Task { task, issue, .. }) = &mut self.dialog else {
            return None;
        };
        Some((task.clone().unwrap_or_default(), issue))
    }

    /// Asks for the issue in full when an editor on a Jira issue opens without it.
    pub(super) fn request_issue(&mut self, fx: &mut Vec<Effect>) {
        if !self.daemon_supports_jira_issues() {
            return;
        }
        let Some((task, _)) = self.issue_editor().filter(|(t, _)| !t.is_empty()) else {
            return;
        };
        let is_issue = self
            .workspace
            .find_task(&task)
            .is_some_and(|f| f.task().issue().is_some());
        let Some((task, editor)) = self.issue_editor() else {
            return;
        };
        if !is_issue || editor.details.is_some() || editor.loading {
            return;
        }
        editor.loading = true;
        let body = Body::GetJiraIssue(v1::GetJiraIssueRequest {
            task_id: task.clone(),
        });
        fx.push(request(body, Followup::JiraIssue(task)));
    }

    /// Makes a change to the issue on screen now; the answer is the issue afterwards.
    fn change_now(&mut self, change: Change, fx: &mut Vec<Effect>) {
        let Some((task, _)) = self.issue_editor().filter(|(t, _)| !t.is_empty()) else {
            return;
        };
        let body = Body::ChangeJiraIssue(v1::ChangeJiraIssueRequest {
            task_id: task.clone(),
            change: Some(change),
        });
        fx.push(request(body, Followup::JiraIssue(task)));
    }

    /// The request that sends an editor's field drafts, if it has any.
    pub(super) fn issue_edits(task: &str, issue: &IssueEditor) -> Option<Body> {
        let edits = issue.edits();
        (!edits.is_empty()).then(|| {
            Body::ChangeJiraIssue(v1::ChangeJiraIssueRequest {
                task_id: task.to_owned(),
                change: Some(Change::Edit(v1::JiraFieldEdits { edits })),
            })
        })
    }

    /// The same, from elsewhere in `update`.
    pub(super) fn update_issue(&mut self, msg: IssueMsg, fx: &mut Vec<Effect>) {
        self.issue_msg(msg, fx);
    }

    /// Handles the editor's Jira messages.
    pub(super) fn issue_msg(&mut self, msg: IssueMsg, fx: &mut Vec<Effect>) {
        match msg {
            IssueMsg::Open(_)
            | IssueMsg::Pick { .. }
            | IssueMsg::Toggle { .. }
            | IssueMsg::AssignToMe
            | IssueMsg::Search => self.issue_field_msg(msg, fx),
            _ => self.issue_change_msg(msg, fx),
        }
    }

    /// Fields and their pickers: drafts, sent with Save.
    fn issue_field_msg(&mut self, msg: IssueMsg, fx: &mut Vec<Effect>) {
        let me = self
            .jira
            .as_ref()
            .map(|a| (a.account_id.clone(), a.display_name.clone()));
        let Some((_, editor)) = self.issue_editor() else {
            return;
        };
        match msg {
            IssueMsg::Open(field) => {
                let opening = field.is_some() && field != editor.open;
                editor.open = field.filter(|_| opening);
                editor.search.clear();
                editor.people.clear();
                editor.issues.clear();
                if opening {
                    self.focus = Some(Field::JiraSearch);
                    self.search_picker(fx);
                }
            }
            IssueMsg::Pick { field, option } => {
                // An issue field (the parent) is written by key.
                let by_key = editor
                    .fields()
                    .iter()
                    .any(|f| f.id == field && f.kind == i32::from(v1::JiraFieldKind::Issue));
                let text = option
                    .as_ref()
                    .filter(|_| by_key)
                    .map(|o| o.id.clone())
                    .unwrap_or_default();
                editor.drafts.insert(
                    field,
                    v1::JiraFieldValue {
                        text,
                        options: option.into_iter().collect(),
                    },
                );
                editor.open = None;
            }
            IssueMsg::Toggle { field, option } => {
                let current = editor
                    .fields()
                    .iter()
                    .find(|f| f.id == field)
                    .map(|f| editor.value(f))
                    .unwrap_or_default();
                let mut value = editor.drafts.get(&field).cloned().unwrap_or(current);
                if let Some(at) = value.options.iter().position(|o| o.id == option.id) {
                    value.options.remove(at);
                } else {
                    value.options.push(option);
                }
                editor.drafts.insert(field, value);
            }
            IssueMsg::AssignToMe => {
                if let Some((id, name)) = me.filter(|(id, _)| !id.is_empty()) {
                    editor.drafts.insert(
                        "assignee".into(),
                        v1::JiraFieldValue {
                            text: String::new(),
                            options: vec![v1::JiraOption { id, name }],
                        },
                    );
                }
            }
            IssueMsg::Search => self.search_picker(fx),
            _ => {}
        }
    }

    /// Changes made at once: status, comments, files, links, watching, work.
    fn issue_change_msg(&mut self, msg: IssueMsg, fx: &mut Vec<Effect>) {
        let Some((task, editor)) = self.issue_editor() else {
            return;
        };
        match msg {
            IssueMsg::Transition(id) => {
                editor.open = None;
                self.change_now(Change::TransitionId(id), fx);
            }
            IssueMsg::Tab(tab) => editor.tab = tab,
            IssueMsg::Comment => {
                let body = editor.comment.text().trim().to_owned();
                if !body.is_empty() {
                    editor.comment = TextField::multiline("");
                    self.change_now(Change::AddComment(body), fx);
                }
            }
            IssueMsg::EditComment(id) => {
                let body = editor
                    .details
                    .as_ref()
                    .and_then(|d| d.comments.iter().find(|c| c.id == id))
                    .map(|c| c.body.clone())
                    .unwrap_or_default();
                editor.editing = Some((id, TextField::multiline(body)));
                self.focus = Some(Field::JiraCommentEdit);
            }
            IssueMsg::SaveComment => {
                if let Some((id, text)) = editor.editing.take() {
                    let change = Change::UpdateComment(v1::JiraCommentEdit {
                        comment_id: id,
                        body: text.text().trim().to_owned(),
                    });
                    self.change_now(change, fx);
                }
            }
            IssueMsg::CancelComment => editor.editing = None,
            IssueMsg::DeleteComment(id) => self.change_now(Change::DeleteCommentId(id), fx),
            IssueMsg::Watch(watch) => self.change_now(Change::Watch(watch), fx),
            IssueMsg::OpenAttachment(id, name) => {
                let body = Body::GetJiraAttachment(v1::GetJiraAttachmentRequest {
                    task_id: task,
                    attachment_id: id.clone(),
                    thumbnail: false,
                });
                fx.push(request(body, Followup::OpenAttachment { id, name }));
            }
            IssueMsg::DeleteAttachment(id) => {
                self.change_now(Change::DeleteAttachmentId(id), fx);
            }
            IssueMsg::LinkKind(i, outward) => editor.link = (i, outward),
            IssueMsg::Link(key) => {
                let link_type = editor
                    .details
                    .as_ref()
                    .and_then(|d| d.link_types.get(editor.link.0))
                    .map(|t| t.name.clone());
                let outward = editor.link.1;
                editor.open = None;
                if let Some(link_type) = link_type {
                    let change = Change::Link(v1::JiraNewLink {
                        link_type,
                        issue_key: key,
                        outward,
                    });
                    self.change_now(change, fx);
                }
            }
            IssueMsg::Unlink(id) => self.change_now(Change::UnlinkId(id), fx),
            IssueMsg::LogWork => match duration(editor.work.text()) {
                Some(seconds) => {
                    editor.work.clear();
                    let change = Change::LogWork(v1::JiraNewWorklog {
                        seconds,
                        started_ms: self.now_ms,
                        comment: String::new(),
                    });
                    self.change_now(change, fx);
                }
                None => {
                    self.error = Some("Type the time spent like 1h 30m, 2d or 45m.".into());
                }
            },
            _ => {}
        }
    }

    /// Searches for the open picker: people for a person field, issues for an issue
    /// field or a new link.
    fn search_picker(&mut self, fx: &mut Vec<Effect>) {
        let board = self.board.board(&self.workspace).map(|b| b.id.clone());
        let Some((task, editor)) = self.issue_editor() else {
            return;
        };
        let Some(open) = editor.open.clone() else {
            return;
        };
        let query = editor.search.text().trim().to_owned();
        let kind = editor
            .fields()
            .iter()
            .find(|f| f.id == open)
            .map(|f| f.kind);
        // A label is added as typed.
        if kind == Some(v1::JiraFieldKind::Labels.into()) {
            if !query.is_empty() {
                editor.search.clear();
                let option = v1::JiraOption {
                    id: query.clone(),
                    name: query,
                };
                self.issue_msg(
                    IssueMsg::Toggle {
                        field: open,
                        option,
                    },
                    fx,
                );
            }
            return;
        }
        let users = [v1::JiraFieldKind::User, v1::JiraFieldKind::Users].map(i32::from);
        let body = if kind.is_some_and(|k| users.contains(&k)) {
            Body::SearchJiraUsers(v1::SearchJiraUsersRequest {
                task_id: task,
                query,
                assignable: open == "assignee",
            })
        } else if open == LINK || kind == Some(v1::JiraFieldKind::Issue.into()) {
            let Some(board_id) = board else {
                return;
            };
            Body::SearchJiraIssues(v1::SearchJiraIssuesRequest { board_id, query })
        } else {
            return;
        };
        fx.push(request(body, Followup::JiraFound(open)));
    }

    /// Answers about issues: an issue in full, or what a picker found. Others are
    /// handed back.
    pub(super) fn issue_answered(
        &mut self,
        followup: Followup,
        outcome: Outcome,
    ) -> Option<(Followup, Outcome)> {
        match (followup, outcome) {
            (Followup::JiraCreateFields(kind), Outcome::JiraIssueDetails(details)) => {
                self.create_fields_answered(kind, details);
            }
            (Followup::JiraIssue(task), Outcome::JiraIssueDetails(details)) => {
                if let Some(editor) = editor_of(&mut self.dialog, &task) {
                    editor.details = Some(details);
                    editor.loading = false;
                }
            }
            (Followup::JiraFound(field), Outcome::JiraUsers(users)) => {
                if let Some((_, editor)) = self.issue_editor()
                    && editor.open.as_deref() == Some(field.as_str())
                {
                    editor.people = users.users;
                }
            }
            (Followup::JiraFound(field), Outcome::JiraIssueRefs(refs)) => {
                if let Some((_, editor)) = self.issue_editor()
                    && editor.open.as_deref() == Some(field.as_str())
                {
                    editor.issues = refs.issues;
                }
            }
            other => return Some(other),
        }
        None
    }

    /// A failed issue request: the editor stops waiting (the banner says why).
    pub(super) fn issue_failed(&mut self, followup: &Followup) {
        if let Followup::JiraIssue(task) = followup
            && let Some(editor) = editor_of(&mut self.dialog, task)
        {
            editor.loading = false;
        }
    }

    /// A file for a Jira issue goes to Jira; any other is handed back.
    pub(super) fn attach_to_issue(
        &mut self,
        task: &str,
        (name, bytes): (String, Vec<u8>),
        fx: &mut Vec<Effect>,
    ) -> Option<(String, Vec<u8>)> {
        let is_issue = self
            .workspace
            .find_task(task)
            .is_some_and(|f| f.task().issue().is_some());
        if !is_issue || !self.daemon_supports_jira_issues() {
            return Some((name, bytes));
        }
        let body = Body::ChangeJiraIssue(v1::ChangeJiraIssueRequest {
            task_id: task.to_owned(),
            change: Some(Change::Attach(v1::JiraNewAttachment { name, data: bytes })),
        });
        fx.push(request(body, Followup::JiraIssue(task.to_owned())));
        None
    }

    /// For a new task on a Jira board: asks which fields an issue of the chosen kind
    /// has (the required ones must be filled in before Save).
    pub(super) fn request_create_fields(&mut self, fx: &mut Vec<Effect>) {
        if !self.daemon_supports_jira_issues() {
            return;
        }
        let Some(Dialog::Task {
            board,
            task: None,
            labels,
            issue,
            ..
        }) = &mut self.dialog
        else {
            return;
        };
        if self.workspace.board(board).is_none_or(|b| b.jira.is_none()) {
            return;
        }
        let kind = if labels.kind == 0 {
            v1::TaskKind::Task.into()
        } else {
            labels.kind
        };
        issue.details = None;
        issue.drafts.clear();
        issue.texts.clear();
        issue.loading = true;
        let body = Body::GetJiraCreateFields(v1::GetJiraCreateFieldsRequest {
            board_id: board.clone(),
            kind,
        });
        fx.push(request(body, Followup::JiraCreateFields(kind)));
    }

    /// A new Jira task's fields arrived (for the kind still chosen).
    /// (A new subtask's editor asks for a Sub-task's; if it was left before the answer,
    /// the task's editor now on screen, of another kind, mustn't take them.)
    fn create_fields_answered(&mut self, kind: i32, details: v1::JiraIssueDetails) {
        if let Some(Dialog::Task {
            task: None,
            labels,
            issue,
            ..
        }) = &mut self.dialog
            && (labels.kind == kind || labels.kind == 0 && kind == i32::from(v1::TaskKind::Task))
        {
            issue.details = Some(details);
            issue.loading = false;
        }
    }

    /// The required fields of a new Jira task still empty, by name.
    pub(super) fn missing_fields(issue: &IssueEditor) -> Vec<String> {
        issue
            .fields()
            .iter()
            .filter(|f| f.required)
            .filter(|f| {
                let v = issue.value(f);
                v.text.trim().is_empty() && v.options.is_empty()
            })
            .map(|f| f.name.clone())
            .collect()
    }
}
