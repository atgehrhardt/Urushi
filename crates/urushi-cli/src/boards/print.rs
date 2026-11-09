//! `urushi board ls`: boards, their columns (with colors) and tasks (with colors, tags,
//! notes, subtasks and threads), as an indented listing.

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
        out(&format!("board {} {}", short(&board.id), board.name))?;
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
                    "    - {}{}{}  [{}]{thread}",
                    task.title,
                    color(task.color),
                    tags(task),
                    short(&task.id)
                ))?;
                for line in task.notes.lines() {
                    out(&format!("        {line}"))?;
                }
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
            "{indent}[{mark}] {}{}{tags}  [{}]{thread}",
            s.title,
            color(s.color),
            short(&s.id)
        ))?;
        for line in s.notes.lines() {
            out(&format!("{indent}    {line}"))?;
        }
        print_subtasks(mirror, &s.subtasks, level + 1)?;
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
