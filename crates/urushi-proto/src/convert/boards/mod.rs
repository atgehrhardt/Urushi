//! Task boards: core ⇄ wire (`v1::Board`, since protocol 1.5; subtasks and thread links
//! since 1.6; colors, tags, column changes, subtasks as full nested tasks and
//! attachments since 1.8; task kinds and Jira boards since 1.12; quick filters since
//! 1.21).

use urushi_core::{Board, Column, Task, ThreadId};

mod attachments;
mod jira;
mod labels;
mod quick_filters;

use attachments::attachment_from_proto;
pub use attachments::attachment_to_proto;
pub use jira::{
    category_to_proto, epic_from_proto, epic_to_proto, issue_from_proto, issue_to_proto,
    kind_from_proto, kind_to_proto, source_from_proto, source_to_proto,
};
pub use labels::{color_from_proto, color_to_proto};
pub use quick_filters::{
    filter_ref_from_proto, filter_ref_to_proto, quick_filter_from_proto, quick_filter_to_proto,
    rule_from_proto, rule_to_proto,
};

use super::{ConvertError, millis_from_proto, millis_to_proto, parse_id};
use crate::v1;

/// Core board → wire.
pub fn board_to_proto(b: &Board) -> v1::Board {
    let site = b.jira().map(|j| j.site.as_str());
    v1::Board {
        id: b.id.to_string(),
        name: b.name.clone(),
        created_at_ms: millis_to_proto(b.created_at),
        columns: b.columns.iter().map(|c| column_on(c, site)).collect(),
        jira: source_to_proto(b.source.as_ref()).map(Box::new),
        epics: b
            .epics
            .iter()
            .map(|e| epic_to_proto(e, site.unwrap_or_default()))
            .collect(),
        quick_filters: b.quick_filters.iter().map(quick_filter_to_proto).collect(),
    }
}

/// Core column → wire (also the answer to `CreateColumn`).
pub fn column_to_proto(c: &Column) -> v1::BoardColumn {
    column_on(c, None)
}

fn column_on(c: &Column, site: Option<&str>) -> v1::BoardColumn {
    v1::BoardColumn {
        id: c.id.to_string(),
        name: c.name.clone(),
        tasks: c.tasks.iter().map(|t| task_on(t, site)).collect(),
        color: color_to_proto(c.color),
        jira_statuses: c.statuses.clone(),
    }
}

/// Core task → wire (also the answer to `CreateTask`). Its subtasks become
/// [`v1::Subtask`]s, which carry the same fields. An issue's web address needs its
/// board: see [`task_in_board_to_proto`].
pub fn task_to_proto(t: &Task) -> v1::Task {
    task_on(t, None)
}

/// A task of `board` → wire, with its issue's web address.
pub fn task_in_board_to_proto(t: &Task, board: &Board) -> v1::Task {
    task_on(t, board.jira().map(|j| j.site.as_str()))
}

/// A subtask of `board` → wire, with its issue's web address.
pub fn subtask_in_board_to_proto(s: &Task, board: &Board) -> v1::Subtask {
    subtask_on(s, board.jira().map(|j| j.site.as_str()))
}

fn task_on(t: &Task, site: Option<&str>) -> v1::Task {
    v1::Task {
        id: t.id.to_string(),
        title: t.title.clone(),
        notes: t.notes.clone(),
        created_at_ms: millis_to_proto(t.created_at),
        subtasks: t.subtasks.iter().map(|s| subtask_on(s, site)).collect(),
        thread_id: link_to_proto(t.thread),
        color: color_to_proto(t.color),
        tags: t.tags.clone(),
        attachments: t.attachments.iter().map(attachment_to_proto).collect(),
        kind: kind_to_proto(t.kind),
        issue: t.issue.as_ref().map(|i| Box::new(issue_to_proto(i, site))),
    }
}

/// A nested core task → wire (also the answer to `CreateSubtask`).
pub fn subtask_to_proto(s: &Task) -> v1::Subtask {
    subtask_on(s, None)
}

fn subtask_on(s: &Task, site: Option<&str>) -> v1::Subtask {
    v1::Subtask {
        id: s.id.to_string(),
        title: s.title.clone(),
        done: s.done,
        thread_id: link_to_proto(s.thread),
        notes: s.notes.clone(),
        created_at_ms: millis_to_proto(s.created_at),
        color: color_to_proto(s.color),
        tags: s.tags.clone(),
        subtasks: s.subtasks.iter().map(|s| subtask_on(s, site)).collect(),
        attachments: s.attachments.iter().map(attachment_to_proto).collect(),
        kind: kind_to_proto(s.kind),
        issue: s.issue.as_ref().map(|i| Box::new(issue_to_proto(i, site))),
    }
}

fn link_to_proto(thread: Option<ThreadId>) -> String {
    thread.map(|t| t.to_string()).unwrap_or_default()
}

fn link_from_proto(id: &str, field: &'static str) -> Result<Option<ThreadId>, ConvertError> {
    if id.is_empty() {
        Ok(None)
    } else {
        parse_id(id, field).map(Some)
    }
}

fn attachments_from_proto(
    attachments: &[v1::Attachment],
) -> Result<Vec<urushi_core::board::Attachment>, ConvertError> {
    attachments.iter().map(attachment_from_proto).collect()
}

fn subtasks_from_proto(subtasks: &[v1::Subtask]) -> Result<Vec<Task>, ConvertError> {
    subtasks.iter().map(subtask_from_proto).collect()
}

fn subtask_from_proto(s: &v1::Subtask) -> Result<Task, ConvertError> {
    Ok(Task {
        id: parse_id(&s.id, "board.task.subtask.id")?,
        title: s.title.clone(),
        notes: s.notes.clone(),
        created_at: millis_from_proto(s.created_at_ms),
        done: s.done,
        color: color_from_proto(s.color),
        tags: s.tags.clone(),
        attachments: attachments_from_proto(&s.attachments)?,
        subtasks: subtasks_from_proto(&s.subtasks)?,
        thread: link_from_proto(&s.thread_id, "board.task.subtask.thread_id")?,
        kind: kind_from_proto(s.kind),
        issue: s.issue.as_deref().map(issue_from_proto),
    })
}

fn task_from_proto(t: &v1::Task) -> Result<Task, ConvertError> {
    Ok(Task {
        id: parse_id(&t.id, "board.task.id")?,
        title: t.title.clone(),
        notes: t.notes.clone(),
        created_at: millis_from_proto(t.created_at_ms),
        done: false,
        color: color_from_proto(t.color),
        tags: t.tags.clone(),
        attachments: attachments_from_proto(&t.attachments)?,
        subtasks: subtasks_from_proto(&t.subtasks)?,
        thread: link_from_proto(&t.thread_id, "board.task.thread_id")?,
        kind: kind_from_proto(t.kind),
        issue: t.issue.as_deref().map(issue_from_proto),
    })
}

/// Wire board → core. Ids are validated; text is taken as sent.
pub fn board_from_proto(b: &v1::Board) -> Result<Board, ConvertError> {
    Ok(Board {
        id: parse_id(&b.id, "board.id")?,
        name: b.name.clone(),
        created_at: millis_from_proto(b.created_at_ms),
        columns: b
            .columns
            .iter()
            .map(|c| {
                Ok(Column {
                    id: parse_id(&c.id, "board.column.id")?,
                    name: c.name.clone(),
                    color: color_from_proto(c.color),
                    tasks: c
                        .tasks
                        .iter()
                        .map(task_from_proto)
                        .collect::<Result<_, ConvertError>>()?,
                    statuses: c.jira_statuses.clone(),
                })
            })
            .collect::<Result<_, ConvertError>>()?,
        source: source_from_proto(b.jira.as_deref()),
        epics: b.epics.iter().map(epic_from_proto).collect(),
        quick_filters: b
            .quick_filters
            .iter()
            .map(quick_filter_from_proto)
            .collect::<Result<_, ConvertError>>()?,
    })
}

#[cfg(test)]
mod tests;
