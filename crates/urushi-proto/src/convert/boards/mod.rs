//! Task boards: core ⇄ wire (`v1::Board`, since protocol 1.5; subtasks and thread links
//! since 1.6; colors, tags, column changes and subtasks as full nested tasks since 1.8).

use urushi_core::{Board, Column, Task, ThreadId};

mod labels;

pub use labels::{color_from_proto, color_to_proto};

use super::{ConvertError, millis_from_proto, millis_to_proto, parse_id};
use crate::v1;

/// Core board → wire.
pub fn board_to_proto(b: &Board) -> v1::Board {
    v1::Board {
        id: b.id.to_string(),
        name: b.name.clone(),
        created_at_ms: millis_to_proto(b.created_at),
        columns: b.columns.iter().map(column_to_proto).collect(),
    }
}

/// Core column → wire (also the answer to `CreateColumn`).
pub fn column_to_proto(c: &Column) -> v1::BoardColumn {
    v1::BoardColumn {
        id: c.id.to_string(),
        name: c.name.clone(),
        tasks: c.tasks.iter().map(task_to_proto).collect(),
        color: color_to_proto(c.color),
    }
}

/// Core task → wire (also the answer to `CreateTask`). Its subtasks become
/// [`v1::Subtask`]s, which carry the same fields.
pub fn task_to_proto(t: &Task) -> v1::Task {
    v1::Task {
        id: t.id.to_string(),
        title: t.title.clone(),
        notes: t.notes.clone(),
        created_at_ms: millis_to_proto(t.created_at),
        subtasks: t.subtasks.iter().map(subtask_to_proto).collect(),
        thread_id: link_to_proto(t.thread),
        color: color_to_proto(t.color),
        tags: t.tags.clone(),
    }
}

/// A nested core task → wire (also the answer to `CreateSubtask`).
pub fn subtask_to_proto(s: &Task) -> v1::Subtask {
    v1::Subtask {
        id: s.id.to_string(),
        title: s.title.clone(),
        done: s.done,
        thread_id: link_to_proto(s.thread),
        notes: s.notes.clone(),
        created_at_ms: millis_to_proto(s.created_at),
        color: color_to_proto(s.color),
        tags: s.tags.clone(),
        subtasks: s.subtasks.iter().map(subtask_to_proto).collect(),
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
        subtasks: subtasks_from_proto(&s.subtasks)?,
        thread: link_from_proto(&s.thread_id, "board.task.subtask.thread_id")?,
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
        subtasks: subtasks_from_proto(&t.subtasks)?,
        thread: link_from_proto(&t.thread_id, "board.task.thread_id")?,
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
                })
            })
            .collect::<Result<_, ConvertError>>()?,
    })
}

#[cfg(test)]
mod tests;
