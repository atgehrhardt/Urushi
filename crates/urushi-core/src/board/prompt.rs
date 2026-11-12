//! The first prompt an agent gets when a thread is started from a task or subtask
//! (ADR 0014, ADR 0024).
//!
//! - [`task_reference_prompt`]: a reference to the task (a Jira issue's key and address,
//!   or the task's id) and how to read it in full with `<cli> board show`, so the
//!   agent reviews the task as it is, comments and links included, rather than a copy.
//!   Used when the agent's pane can run the CLI. The CLI is named by its path, as a
//!   login shell may put another `urushi` first on `PATH` (ADR 0033).
//! - [`task_prompt`]: the task's title, notes and subtasks, and for a subtask the tasks
//!   it belongs to, as plain text; when it can't.
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

/// The command that prints a task in full with the CLI `cli` (quoted for a shell as
/// needed): by issue key on a Jira board, else by id.
pub(super) fn show_command(task: &Task, cli: &str) -> String {
    let id = task
        .issue
        .as_ref()
        .map_or_else(|| task.id.to_string(), |i| i.key.clone());
    format!("{} board show {id}", crate::shell::quote(cli))
}

/// How a task is named in a reference: `UR-1 "Title"`, or `"Title"`.
fn named(task: &Task) -> String {
    match &task.issue {
        Some(issue) => format!("{} \"{}\"", issue.key, task.title),
        None => format!("\"{}\"", task.title),
    }
}

/// A prompt that refers to task `id` of `board` (at any depth) and says how to review it
/// in full with Urushi's CLI, instead of copying it. `cli` is the program that runs the
/// CLI in the agent's pane: its absolute path, so no other `urushi` on `PATH` is run.
pub fn task_reference_prompt(board: &Board, id: TaskId, cli: &str) -> Result<String, BoardError> {
    let path = board.path_to(id).ok_or(BoardError::NoSuchTask(id))?;
    let (task, ancestors) = path.split_last().ok_or(BoardError::NoSuchTask(id))?;
    let mut out = String::new();
    let what = if task.issue.is_some() {
        "Jira issue"
    } else if ancestors.is_empty() {
        "the task"
    } else {
        "the step"
    };
    let _ = write!(out, "Work on {what} {} from my Urushi board", named(task));
    match (board.jira(), &task.issue) {
        (Some(jira), Some(issue)) => {
            let _ = writeln!(out, ": {}", jira.issue_url(&issue.key));
        }
        _ => out.push_str(".\n"),
    }
    if let Some(parent) = ancestors.last() {
        let _ = writeln!(out, "It is part of {}.", named(parent));
    }
    let _ = write!(
        out,
        "\nStart by reviewing it in full: run `{}`",
        show_command(task, cli)
    );
    if let Some(parent) = ancestors.last() {
        let _ = write!(
            out,
            " (and `{}` for the task it is part of)",
            show_command(parent, cli)
        );
    }
    out.push_str(". It prints ");
    out.push_str(if task.issue.is_some() {
        "the issue's fields, description, subtasks, comments, links and files."
    } else {
        "its notes, subtasks and attachments."
    });
    if let Some(issue) = &task.issue {
        out.push_str(" Ask me before changing anything in Jira.");
        // Jira links branches, commits and pull requests by the key in them (ADR 0026).
        let _ = write!(
            out,
            " Put {} in commit messages and pull request titles, so Jira links them to \
             the issue.",
            issue.key
        );
    }
    Ok(capped(out))
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

pub(super) fn capped(mut prompt: String) -> String {
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

    #[test]
    fn a_reference_says_where_the_task_is_and_how_to_read_it() {
        let b = board("Cards need tags.");
        let card = task_reference_prompt(&b, TaskId::from_u128(1), "/opt/urushi/urushi").unwrap();
        assert_eq!(
            card,
            format!(
                "Work on the task \"Add tags\" from my Urushi board.\n\n\
                 Start by reviewing it in full: run `/opt/urushi/urushi board show {}`. It prints its \
                 notes, subtasks and attachments.",
                TaskId::from_u128(1)
            )
        );
        assert!(
            !card.contains("Cards need tags."),
            "a reference, not a copy"
        );
        // A CLI whose path the shell would split is quoted.
        let step = task_reference_prompt(&b, TaskId::from_u128(12), "/My Apps/urushi").unwrap();
        assert!(step.starts_with("Work on the step \"Chips\""), "{step}");
        assert!(step.contains("It is part of \"UI\"."), "{step}");
        assert!(
            step.contains(&format!(
                "(and `'/My Apps/urushi' board show {}`",
                TaskId::from_u128(11)
            )),
            "{step}"
        );
    }

    #[test]
    fn a_jira_issue_is_referred_to_by_key_and_address() {
        use crate::board::{Issue, JiraBoard, JiraColumn, SyncedIssue, TaskKind};
        let mut b = Board::new_jira(
            crate::BoardId::from_u128(5),
            JiraBoard {
                site: "https://acme.atlassian.net".into(),
                account: String::new(),
                board_id: 34,
                name: "UR".into(),
                project_key: "UR".into(),
                scrum: false,
                rank_field: None,
                points_field: None,
                sprint_field: None,
                quick_filters: Vec::new(),
                sync: crate::board::SyncState::default(),
            },
            &[JiraColumn {
                name: "To Do".into(),
                statuses: vec!["1".into()],
            }],
            Timestamp::from_unix_millis(0),
            || ColumnId::from_u128(50),
        )
        .unwrap();
        let synced = |n: u32, parent: Option<u32>| SyncedIssue {
            issue: Issue {
                id: format!("{n}"),
                key: format!("UR-{n}"),
                status_id: "1".into(),
                parent: parent.map(|p| format!("{p}")),
                ..Issue::default()
            },
            title: format!("Issue {n}"),
            notes: "Long notes".into(),
            labels: Vec::new(),
            kind: if parent.is_some() {
                TaskKind::Subtask
            } else {
                TaskKind::Story
            },
            epic_summary: None,
            created: Timestamp::from_unix_millis(0),
        };
        let mut next = 60u128;
        b.merge_issues(vec![synced(1, None), synced(3, Some(1))], true, || {
            next += 1;
            TaskId::from_u128(next)
        });
        let sub = b.task_for_issue("UR-3").unwrap().id;
        let cli = "/Applications/Urushi.app/Contents/Resources/bin/urushi";
        let p = task_reference_prompt(&b, sub, cli).unwrap();
        assert_eq!(
            p,
            format!(
                "Work on Jira issue UR-3 \"Issue 3\" from my Urushi board: \
                 https://acme.atlassian.net/browse/UR-3\n\
                 It is part of UR-1 \"Issue 1\".\n\n\
                 Start by reviewing it in full: run `{cli} board show UR-3` (and `{cli} \
                 board show UR-1` for the task it is part of). It prints the issue's fields, \
                 description, subtasks, comments, links and files. Ask me before changing \
                 anything in Jira. Put UR-3 in commit messages and pull request titles, so \
                 Jira links them to the issue."
            )
        );
    }
}
