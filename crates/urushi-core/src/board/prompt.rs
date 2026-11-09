//! The first prompt an agent gets when a thread is started from a task or subtask: the
//! task's title, notes and checklist, as plain text (ADR 0014).
//!
//! The daemon passes it to the agent through the pane's environment, so it is capped at
//! [`MAX_PROMPT_BYTES`], well under the per-string limits operating systems put on
//! environment variables and arguments.

use std::fmt::Write as _;

use super::{BoardError, Subtask, Task};
use crate::SubtaskId;

/// Longest prompt, in bytes. Longer prompts are cut at a character boundary.
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;

/// What was cut from a prompt that ran over [`MAX_PROMPT_BYTES`].
const CUT_NOTICE: &str = "\n\n[The rest of this task was cut to fit.]";

/// The prompt for `task`, or for its subtask `subtask`.
///
/// For a subtask, the task's notes and the whole checklist come along as context, with
/// the step to work on marked, so the agent knows where the step fits.
pub fn task_prompt(task: &Task, subtask: Option<SubtaskId>) -> Result<String, BoardError> {
    let mut out = String::new();
    match subtask {
        None => {
            out.push_str("Work on this task from my Urushi board.\n\n");
            let _ = writeln!(out, "Task: {}", task.title);
            push_notes(&mut out, "Notes", &task.notes);
            if !task.subtasks.is_empty() {
                out.push_str("\nSubtasks:\n");
                push_checklist(&mut out, &task.subtasks, None);
            }
        }
        Some(id) => {
            let step = task
                .subtasks
                .iter()
                .find(|s| s.id == id)
                .ok_or(BoardError::NoSuchSubtask(id))?;
            out.push_str("Work on this step of a task from my Urushi board.\n\n");
            let _ = writeln!(out, "Step: {}", step.title);
            let _ = writeln!(out, "\nIt is part of the task: {}", task.title);
            push_notes(&mut out, "Task notes", &task.notes);
            out.push_str("\nAll steps of the task (for context; work only on the step above):\n");
            push_checklist(&mut out, &task.subtasks, Some(id));
        }
    }
    Ok(capped(out.trim_end().to_owned()))
}

fn push_notes(out: &mut String, heading: &str, notes: &str) {
    let notes = notes.trim();
    if !notes.is_empty() {
        let _ = write!(out, "\n{heading}:\n{notes}\n");
    }
}

fn push_checklist(out: &mut String, subtasks: &[Subtask], current: Option<SubtaskId>) {
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
    use crate::{TaskId, Timestamp};

    fn task(notes: &str, steps: &[(&str, bool)]) -> Task {
        let mut t = Task::new(
            TaskId::from_u128(1),
            "Add tags",
            notes,
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        for (i, (title, done)) in steps.iter().enumerate() {
            let mut s = Subtask::new(SubtaskId::from_u128(10 + i as u128), title).unwrap();
            s.done = *done;
            t.subtasks.push(s);
        }
        t
    }

    #[test]
    fn a_task_brings_its_notes_and_checklist() {
        let t = task(
            "Cards need tags.\nKeep it simple.",
            &[("Model", true), ("UI", false)],
        );
        assert_eq!(
            task_prompt(&t, None).unwrap(),
            "Work on this task from my Urushi board.\n\n\
             Task: Add tags\n\n\
             Notes:\nCards need tags.\nKeep it simple.\n\n\
             Subtasks:\n- [x] Model\n- [ ] UI"
        );
    }

    #[test]
    fn a_bare_task_is_just_its_title() {
        assert_eq!(
            task_prompt(&task("  ", &[]), None).unwrap(),
            "Work on this task from my Urushi board.\n\nTask: Add tags"
        );
    }

    #[test]
    fn a_subtask_is_the_step_with_its_task_as_context() {
        let t = task("Cards need tags.", &[("Model", true), ("UI", false)]);
        assert_eq!(
            task_prompt(&t, Some(SubtaskId::from_u128(11))).unwrap(),
            "Work on this step of a task from my Urushi board.\n\n\
             Step: UI\n\n\
             It is part of the task: Add tags\n\n\
             Task notes:\nCards need tags.\n\n\
             All steps of the task (for context; work only on the step above):\n\
             - [x] Model\n- [ ] UI  <- this step"
        );
        assert_eq!(
            task_prompt(&t, Some(SubtaskId::from_u128(99))),
            Err(BoardError::NoSuchSubtask(SubtaskId::from_u128(99)))
        );
    }

    #[test]
    fn long_prompts_are_cut_at_a_character_boundary() {
        let mut t = task("", &[]);
        t.notes = "é".repeat(MAX_PROMPT_BYTES);
        let p = task_prompt(&t, None).unwrap();
        assert!(p.len() <= MAX_PROMPT_BYTES);
        assert!(p.ends_with(CUT_NOTICE));
        assert!(p.starts_with("Work on this task"));
    }
}
