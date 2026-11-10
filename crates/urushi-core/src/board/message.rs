//! A task's prompt from the user's own message (UR-30): the message with its placeholders
//! (`agent::TASK_MESSAGE_PLACEHOLDERS`) filled in for one task.

use super::prompt::{capped, show_command};
use super::{Board, BoardError};
use crate::TaskId;

/// The user's `template` filled in for task `id` of `board` (at any depth):
/// `{key}`, `{title}`, `{url}` and `{show}` (the command that prints the task with the
/// CLI `cli`, as in `task_reference_prompt`). Anything else in braces is kept as
/// written, and text filled in is never read again for placeholders. Capped like the
/// default prompts.
pub fn custom_task_prompt(
    board: &Board,
    id: TaskId,
    template: &str,
    cli: &str,
) -> Result<String, BoardError> {
    let task = board.task(id).ok_or(BoardError::NoSuchTask(id))?;
    let key = task
        .issue
        .as_ref()
        .map_or_else(|| task.id.to_string(), |i| i.key.clone());
    let url = match (board.jira(), &task.issue) {
        (Some(jira), Some(issue)) => jira.issue_url(&issue.key),
        _ => String::new(),
    };
    let show = show_command(task, cli);
    let value = |name: &str| match name {
        "key" => Some(key.as_str()),
        "title" => Some(task.title.as_str()),
        "url" => Some(url.as_str()),
        "show" => Some(show.as_str()),
        _ => None,
    };
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let placeholder = after
            .find('}')
            .and_then(|close| Some((value(&after[..close])?, close)));
        if let Some((filled, close)) = placeholder {
            out.push_str(filled);
            rest = &after[close + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    Ok(capped(out.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Issue, JiraBoard, JiraColumn, SyncState, SyncedIssue, Task, TaskKind};
    use crate::{BoardId, ColumnId, Timestamp};

    fn plain_board() -> Board {
        let mut b = Board::new(
            BoardId::from_u128(1),
            "b",
            Timestamp::from_unix_millis(0),
            [2, 3, 4].map(ColumnId::from_u128),
        )
        .unwrap();
        let t = Task::new(
            TaskId::from_u128(7),
            "Add {tags}",
            "",
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        b.add_task(ColumnId::from_u128(2), t).unwrap();
        b
    }

    fn jira_board() -> (Board, TaskId) {
        let mut b = Board::new_jira(
            BoardId::from_u128(5),
            JiraBoard {
                site: "https://acme.atlassian.net".into(),
                board_id: 34,
                name: "UR".into(),
                project_key: "UR".into(),
                scrum: false,
                rank_field: None,
                points_field: None,
                sprint_field: None,
                quick_filters: Vec::new(),
                sync: SyncState::default(),
            },
            &[JiraColumn {
                name: "To Do".into(),
                statuses: vec!["1".into()],
            }],
            Timestamp::from_unix_millis(0),
            || ColumnId::from_u128(50),
        )
        .unwrap();
        let issue = SyncedIssue {
            issue: Issue {
                id: "30".into(),
                key: "UR-30".into(),
                status_id: "1".into(),
                ..Issue::default()
            },
            title: "Task Starting".into(),
            notes: String::new(),
            labels: Vec::new(),
            kind: TaskKind::Task,
            epic_summary: None,
            created: Timestamp::from_unix_millis(0),
        };
        b.merge_issues(vec![issue], true, || TaskId::from_u128(60));
        let id = b.task_for_issue("UR-30").unwrap().id;
        (b, id)
    }

    #[test]
    fn a_jira_issue_fills_in_every_placeholder() {
        let (b, id) = jira_board();
        assert_eq!(
            custom_task_prompt(
                &b,
                id,
                "Pick up {key} \"{title}\" ({url}).\nRead it with `{show}`.\n",
                "/opt/urushi/urushi"
            )
            .unwrap(),
            "Pick up UR-30 \"Task Starting\" (https://acme.atlassian.net/browse/UR-30).\n\
             Read it with `/opt/urushi/urushi board show UR-30`."
        );
    }

    #[test]
    fn a_plain_task_uses_its_id_and_has_no_address() {
        let b = plain_board();
        let id = TaskId::from_u128(7);
        assert_eq!(
            custom_task_prompt(&b, id, "{key}|{url}|{show}", "urushi").unwrap(),
            format!("{id}||urushi board show {id}")
        );
        assert_eq!(
            custom_task_prompt(&b, id, "{show}", "/it's here/urushi").unwrap(),
            format!("'/it'\\''s here/urushi' board show {id}"),
            "a CLI path is quoted for the shell"
        );
    }

    #[test]
    fn other_braces_stay_and_filled_text_is_not_read_again() {
        let b = plain_board();
        let id = TaskId::from_u128(7);
        assert_eq!(
            custom_task_prompt(&b, id, "{title} {nope} {url {", "urushi").unwrap(),
            "Add {tags} {nope} {url {"
        );
        assert_eq!(
            custom_task_prompt(&b, id, "no placeholders", "urushi").unwrap(),
            "no placeholders"
        );
        assert_eq!(
            custom_task_prompt(&b, TaskId::from_u128(99), "x", "urushi"),
            Err(BoardError::NoSuchTask(TaskId::from_u128(99)))
        );
    }
}
