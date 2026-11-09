//! The first prompt an agent gets when a thread is started from a task or subtask: the
//! task's title, notes and subtasks, and for a subtask the tasks it belongs to, as plain
//! text (ADR 0014).
//!
//! The daemon passes it to the agent through the pane's environment, so it is capped at
//! [`MAX_PROMPT_BYTES`], well under the per-string limits operating systems put on
//! environment variables and arguments.

use std::fmt::Write as _;

use super::{Board, BoardError, Task};
use crate::TaskId;

/// Longest prompt, in bytes. Longer prompts are cut at a character boundary.
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;

/// What was cut from a prompt that ran over [`MAX_PROMPT_BYTES`].
const CUT_NOTICE: &str = "\n\n[The rest of this task was cut to fit.]";

/// The prompt for the task `id` of `board`, at any depth.
///
/// A card brings its notes and its subtasks. A subtask also brings the tasks it is part
/// of, and its parent's notes and steps with this one marked, so the agent knows where
/// the step fits.
pub fn task_prompt(board: &Board, id: TaskId) -> Result<String, BoardError> {
    let path = board.path_to(id).ok_or(BoardError::NoSuchTask(id))?;
    let (task, ancestors) = path.split_last().ok_or(BoardError::NoSuchTask(id))?;
    let mut out = String::new();
    match ancestors.last() {
        None => {
            out.push_str("Work on this task from my Urushi board.\n\n");
            let _ = writeln!(out, "Task: {}", task.title);
            push_notes(&mut out, "Notes", &task.notes);
            push_subtasks(&mut out, task);
        }
        Some(parent) => {
            out.push_str("Work on this step of a task from my Urushi board.\n\n");
            let _ = writeln!(out, "Step: {}", task.title);
            push_notes(&mut out, "Notes", &task.notes);
            push_subtasks(&mut out, task);
            let chain: Vec<&str> = ancestors.iter().map(|t| t.title.as_str()).collect();
            let _ = writeln!(out, "\nIt is part of the task: {}", chain.join(" › "));
            push_notes(&mut out, "Task notes", &parent.notes);
            out.push_str("\nAll steps of the task (for context; work only on the step above):\n");
            push_checklist(&mut out, &parent.subtasks, Some(id));
        }
    }
    Ok(capped(out.trim_end().to_owned()))
}

fn push_subtasks(out: &mut String, task: &Task) {
    if !task.subtasks.is_empty() {
        out.push_str("\nSubtasks:\n");
        push_checklist(out, &task.subtasks, None);
    }
}

fn push_notes(out: &mut String, heading: &str, notes: &str) {
    let notes = notes.trim();
    if !notes.is_empty() {
        let _ = write!(out, "\n{heading}:\n{notes}\n");
    }
}

fn push_checklist(out: &mut String, subtasks: &[Task], current: Option<TaskId>) {
    for s in subtasks {
        let mark = if s.done { "x" } else { " " };
        let here = if Some(s.id) == current {
            "  <- this step"
        } else {
            ""
        };
        let _ = writeln!(out, "- [{mark}] {}{here}", s.title);
    }
}

fn capped(mut prompt: String) -> String {
    if prompt.len() <= MAX_PROMPT_BYTES {
        return prompt;
    }
    let mut end = MAX_PROMPT_BYTES - CUT_NOTICE.len();
    while !prompt.is_char_boundary(end) {
        end -= 1;
    }
    prompt.truncate(end);
    prompt.push_str(CUT_NOTICE);
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoardId, ColumnId, Timestamp};

    fn task(id: u128, title: &str, notes: &str) -> Task {
        Task::new(
            TaskId::from_u128(id),
            title,
            notes,
            Timestamp::from_unix_millis(0),
        )
        .unwrap()
    }

    /// A board with "Add tags" (steps "Model" ✓ and "UI") on it; "UI" has a step "Chips".
    fn board(notes: &str) -> Board {
        let mut b = Board::new(
            BoardId::from_u128(100),
            "b",
            Timestamp::from_unix_millis(0),
            [101, 102, 103].map(ColumnId::from_u128),
        )
        .unwrap();
        let mut t = task(1, "Add tags", notes);
        let mut model = task(10, "Model", "");
        model.done = true;
        let mut ui = task(11, "UI", "Chips on cards.");
        ui.subtasks.push(task(12, "Chips", ""));
        t.subtasks = vec![model, ui];
        b.add_task(ColumnId::from_u128(101), t).unwrap();
        b
    }

    #[test]
    fn a_task_brings_its_notes_and_checklist() {
        assert_eq!(
            task_prompt(
                &board("Cards need tags.\nKeep it simple."),
                TaskId::from_u128(1)
            )
            .unwrap(),
            "Work on this task from my Urushi board.\n\n\
             Task: Add tags\n\n\
             Notes:\nCards need tags.\nKeep it simple.\n\n\
             Subtasks:\n- [x] Model\n- [ ] UI"
        );
    }

    #[test]
    fn a_bare_task_is_just_its_title() {
        let mut b = board("  ");
        b.remove_task(TaskId::from_u128(1)).unwrap();
        b.add_task(ColumnId::from_u128(101), task(2, "Add tags", "  "))
            .unwrap();
        assert_eq!(
            task_prompt(&b, TaskId::from_u128(2)).unwrap(),
            "Work on this task from my Urushi board.\n\nTask: Add tags"
        );
    }

    #[test]
    fn a_subtask_is_the_step_with_its_task_as_context() {
        let b = board("Cards need tags.");
        assert_eq!(
            task_prompt(&b, TaskId::from_u128(11)).unwrap(),
            "Work on this step of a task from my Urushi board.\n\n\
             Step: UI\n\n\
             Notes:\nChips on cards.\n\n\
             Subtasks:\n- [ ] Chips\n\n\
             It is part of the task: Add tags\n\n\
             Task notes:\nCards need tags.\n\n\
             All steps of the task (for context; work only on the step above):\n\
             - [x] Model\n- [ ] UI  <- this step"
        );
        assert_eq!(
            task_prompt(&b, TaskId::from_u128(99)),
            Err(BoardError::NoSuchTask(TaskId::from_u128(99)))
        );
    }

    #[test]
    fn a_deeper_step_names_every_task_above_it() {
        let p = task_prompt(&board(""), TaskId::from_u128(12)).unwrap();
        assert!(
            p.starts_with("Work on this step of a task from my Urushi board.\n\nStep: Chips"),
            "{p}"
        );
        assert!(
            p.contains("It is part of the task: Add tags › UI\n\nTask notes:\nChips on cards."),
            "{p}"
        );
        assert!(p.ends_with("- [ ] Chips  <- this step"), "{p}");
    }

    #[test]
    fn long_prompts_are_cut_at_a_character_boundary() {
        let mut b = board("");
        b.remove_task(TaskId::from_u128(1)).unwrap();
        let mut long = task(2, "Add tags", "");
        long.notes = "é".repeat(MAX_PROMPT_BYTES);
        b.add_task(ColumnId::from_u128(101), long).unwrap();
        let p = task_prompt(&b, TaskId::from_u128(2)).unwrap();
        assert!(p.len() <= MAX_PROMPT_BYTES);
        assert!(p.ends_with(CUT_NOTICE));
        assert!(p.starts_with("Work on this task"));
    }
}
