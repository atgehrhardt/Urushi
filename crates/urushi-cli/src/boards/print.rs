//! `urushi board ls`: boards, their columns (with colors) and tasks (with colors, tags,
//! notes, subtasks and threads), as an indented listing. Jira boards say where they
//! come from and how their sync went; their tasks show issue keys, kinds, epics and
//! assignees.

use anyhow::Result;
use urushi_client::{WorkspaceMirror, v1};

use crate::commands::out;
use crate::render::short;

/// " (red)" for a colored column or task; nothing without a color.
fn color(value: i32) -> String {
    urushi_proto::convert::color_from_proto(value)
        .map(|c| format!(" ({})", c.name()))
        .unwrap_or_default()
}

/// "ACME-12 story · " for a task's issue and kind; "story · " for a kind alone.
fn issue(kind: i32, issue: Option<&v1::JiraIssue>) -> String {
    let kind = urushi_proto::convert::kind_from_proto(kind).map(urushi_core::TaskKind::name);
    match (issue, kind) {
        (Some(i), Some(k)) => format!("{} {k} · ", i.key),
        (Some(i), None) => format!("{} · ", i.key),
        (None, Some(k)) => format!("{k} · "),
        (None, None) => String::new(),
    }
}

/// "  epic ACME-7 Billing  @Ada  !High" for an issue's epic, assignee and priority.
fn issue_details(board: &v1::Board, issue: Option<&v1::JiraIssue>) -> String {
    let Some(i) = issue else {
        return String::new();
    };
    let epic = board
        .epics
        .iter()
        .find(|e| e.key == i.epic_key)
        .map(|e| format!("  epic {} {}", e.key, e.summary));
    let assignee = (!i.assignee.is_empty()).then(|| format!("  @{}", i.assignee));
    let priority = (!i.priority.is_empty()).then(|| format!("  !{}", i.priority));
    [epic, assignee, priority].into_iter().flatten().collect()
}

/// " · Jira ACME board (synced …)" for a Jira board.
fn source(board: &v1::Board) -> String {
    let Some(j) = board.jira.as_deref() else {
        return String::new();
    };
    let state = if !j.error.is_empty() {
        format!("sync failed: {}", j.error)
    } else if j.synced_at_ms == 0 {
        "not synced yet".into()
    } else {
        "synced".into()
    };
    let dropped = if j.dropped > 0 {
        format!(", {} issues not shown", j.dropped)
    } else {
        String::new()
    };
    // Which sign-in syncs it (1.29), when the daemon says.
    let account = if j.account.is_empty() {
        String::new()
    } else {
        format!(" as {}", j.account)
    };
    format!(
        "  · Jira {} at {}{account} ({state}{dropped})",
        j.name, j.site
    )
}

/// "  #ui #backend" for a task's tags.
fn tags(task: &v1::Task) -> String {
    task.tags.iter().fold(String::new(), |mut out, t| {
        out.push_str("  #");
        out.push_str(t);
        out
    })
}

pub(super) fn print_boards(mirror: &WorkspaceMirror) -> Result<()> {
    if mirror.boards().is_empty() {
        return out("no boards (create one with `urushi board new <name>`)");
    }
    for board in mirror.boards() {
        out(&format!(
            "board {} {}{}",
            short(&board.id),
            board.name,
            source(board)
        ))?;
        for column in &board.columns {
            out(&format!(
                "  {}{} ({})  [{}]",
                column.name,
                color(column.color),
                column.tasks.len(),
                short(&column.id)
            ))?;
            for task in &column.tasks {
                let thread = link(mirror, &task.thread_id);
                out(&format!(
                    "    - {}{}{}{}{}  [{}]{thread}",
                    issue(task.kind, task.issue.as_deref()),
                    task.title,
                    color(task.color),
                    tags(task),
                    issue_details(board, task.issue.as_deref()),
                    short(&task.id)
                ))?;
                for line in task.notes.lines() {
                    out(&format!("        {line}"))?;
                }
                print_attachments(&task.attachments, "        ")?;
                print_subtasks(mirror, &task.subtasks, 2)?;
            }
        }
    }
    Ok(())
}

/// Subtasks as a checklist, each level indented further, with their own notes,
/// colors, tags and subtasks.
fn print_subtasks(mirror: &WorkspaceMirror, subtasks: &[v1::Subtask], level: usize) -> Result<()> {
    let indent = "    ".repeat(level);
    for s in subtasks {
        let mark = if s.done { "x" } else { " " };
        let thread = link(mirror, &s.thread_id);
        let tags: String = s.tags.iter().fold(String::new(), |mut out, t| {
            out.push_str("  #");
            out.push_str(t);
            out
        });
        out(&format!(
            "{indent}[{mark}] {}{}{}{tags}  [{}]{thread}",
            issue(s.kind, s.issue.as_deref()),
            s.title,
            color(s.color),
            short(&s.id)
        ))?;
        for line in s.notes.lines() {
            out(&format!("{indent}    {line}"))?;
        }
        print_attachments(&s.attachments, &format!("{indent}    "))?;
        print_subtasks(mirror, &s.subtasks, level + 1)?;
    }
    Ok(())
}

/// "📎 name (type, size)  [id]" for each attachment.
fn print_attachments(attachments: &[v1::Attachment], indent: &str) -> Result<()> {
    for a in attachments {
        let size = if a.width > 0 {
            format!("{}×{}, {} bytes", a.width, a.height, a.size)
        } else {
            format!("{} bytes", a.size)
        };
        out(&format!(
            "{indent}📎 {} ({}, {size})  [{}]",
            a.name,
            a.media_type,
            short(&a.id)
        ))?;
    }
    Ok(())
}

/// " → thread <title>" for a task's or subtask's thread, if it still exists.
fn link(mirror: &WorkspaceMirror, thread: &str) -> String {
    mirror
        .thread(thread)
        .map(|t| format!("  → thread {} [{}]", t.title, short(&t.id)))
        .unwrap_or_default()
}
