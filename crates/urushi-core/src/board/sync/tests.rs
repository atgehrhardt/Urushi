//! Merging Jira snapshots into a board, and what a Jira board refuses.

use super::*;
use crate::board::{LabelColor, PrStatus, PrUrl, PullRequest, TaskKind, TaskPatch};
use crate::{ThreadId, Timestamp};

const TODO: &str = "1";
const DOING: &str = "3";
const DONE: &str = "10001";

fn jira_board() -> Board {
    let mut next = 0u128;
    Board::new_jira(
        BoardId::from_u128(1),
        JiraBoard {
            site: "https://acme.atlassian.net".into(),
            board_id: 7,
            name: "ACME   board".into(),
            project_key: "ACME".into(),
            scrum: false,
            rank_field: None,
            points_field: None,
            sprint_field: None,
            quick_filters: Vec::new(),
            sync: crate::board::SyncState::default(),
        },
        &[
            column("To Do", &[TODO]),
            column("In Progress", &[DOING]),
            column("Done", &[DONE]),
        ],
        Timestamp::from_unix_millis(1),
        || {
            next += 1;
            ColumnId::from_u128(10 + next)
        },
    )
    .unwrap()
}

fn column(name: &str, statuses: &[&str]) -> JiraColumn {
    JiraColumn {
        name: name.into(),
        statuses: statuses.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn issue(n: u32, status: &str, rank: &str) -> SyncedIssue {
    SyncedIssue {
        issue: Issue {
            id: format!("{}", 10_000 + n),
            key: format!("ACME-{n}"),
            type_name: "Story".into(),
            status_id: status.into(),
            status: status.into(),
            category: if status == DONE {
                StatusCategory::Done
            } else {
                StatusCategory::ToDo
            },
            rank: rank.into(),
            notes_editable: true,
            ..Issue::default()
        },
        title: format!("Issue {n}"),
        notes: String::new(),
        labels: Vec::new(),
        kind: TaskKind::Story,
        epic_summary: None,
        created: Timestamp::from_unix_millis(2),
    }
}

fn subtask(n: u32, parent: u32, status: &str) -> SyncedIssue {
    let mut s = issue(n, status, &format!("0|s{n:04}"));
    s.kind = TaskKind::Subtask;
    s.issue.parent = Some(format!("{}", 10_000 + parent));
    s
}

fn ids() -> impl FnMut() -> TaskId {
    let mut next = 100u128;
    move || {
        next += 1;
        TaskId::from_u128(next)
    }
}

fn keys(board: &Board, column: usize) -> Vec<&str> {
    board.columns[column]
        .tasks
        .iter()
        .map(|t| t.issue.as_ref().unwrap().key.as_str())
        .collect()
}

#[test]
fn a_new_jira_board_has_the_jira_columns_and_no_tasks() {
    let b = jira_board();
    assert_eq!(b.name, "ACME board");
    let names: Vec<_> = b.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["To Do", "In Progress", "Done"]);
    assert_eq!(b.columns[2].statuses, [DONE]);
    assert_eq!(
        b.jira().unwrap().issue_url("ACME-1"),
        "https://acme.atlassian.net/browse/ACME-1"
    );
}

#[test]
fn issues_go_to_their_status_column_in_rank_order() {
    let mut b = jira_board();
    let report = b.merge_issues(
        vec![
            issue(1, TODO, "0|b"),
            issue(2, TODO, "0|a"),
            issue(3, DONE, "0|c"),
            issue(4, "999", "0|d"), // a status no column shows
        ],
        true,
        ids(),
    );
    assert!(report.changed);
    assert_eq!(report.dropped, 0);
    assert_eq!(keys(&b, 0), ["ACME-2", "ACME-1"]);
    assert_eq!(keys(&b, 2), ["ACME-3"]);
    let t = b.task_for_issue("ACME-1").unwrap();
    assert_eq!(t.kind, Some(TaskKind::Story));
    assert_eq!(t.title, "Issue 1");
}

#[test]
fn merging_the_same_snapshot_again_changes_nothing() {
    let mut b = jira_board();
    let snapshot = vec![issue(1, TODO, "0|a"), subtask(2, 1, DONE)];
    b.merge_issues(snapshot.clone(), true, ids());
    let before = b.clone();
    let report = b.merge_issues(snapshot, true, ids());
    assert!(!report.changed);
    assert_eq!(b, before);
}

#[test]
fn a_task_keeps_its_id_link_and_color_when_its_issue_changes() {
    let mut b = jira_board();
    b.merge_issues(vec![issue(1, TODO, "0|a")], true, ids());
    let id = b.task_for_issue("ACME-1").unwrap().id;
    b.link_thread(id, ThreadId::from_u128(9)).unwrap();
    b.update_task(
        id,
        &TaskPatch {
            color: Some(Some(LabelColor::Red)),
            ..TaskPatch::default()
        },
    )
    .unwrap();

    let mut moved = issue(1, DOING, "0|a");
    moved.title = "Renamed in Jira".into();
    moved.labels = vec!["backend".into(), "has space".into()];
    b.merge_issues(vec![moved], false, ids());

    let t = b.task_for_issue("ACME-1").unwrap();
    assert_eq!(t.id, id);
    assert_eq!(t.thread, Some(ThreadId::from_u128(9)));
    assert_eq!(t.color, Some(LabelColor::Red));
    assert_eq!(t.title, "Renamed in Jira");
    assert_eq!(t.tags, ["backend"], "labels a tag can't hold are left out");
    assert_eq!(b.column_of(id), Some(b.columns[1].id));
}

#[test]
fn a_partial_snapshot_keeps_the_rest_and_a_full_one_removes_the_missing() {
    let mut b = jira_board();
    b.merge_issues(
        vec![issue(1, TODO, "0|a"), issue(2, TODO, "0|b")],
        true,
        ids(),
    );
    b.merge_issues(vec![issue(3, TODO, "0|c")], false, ids());
    assert_eq!(keys(&b, 0), ["ACME-1", "ACME-2", "ACME-3"]);
    b.merge_issues(
        vec![issue(1, TODO, "0|a"), issue(3, TODO, "0|c")],
        true,
        ids(),
    );
    assert_eq!(keys(&b, 0), ["ACME-1", "ACME-3"]);
}

#[test]
fn jira_subtasks_nest_under_their_parent_and_are_checked_when_done() {
    let mut b = jira_board();
    b.merge_issues(
        vec![
            subtask(3, 1, DONE),
            subtask(2, 1, TODO),
            issue(1, DOING, "0|a"),
            subtask(5, 4, TODO), // its parent isn't on the board
        ],
        true,
        ids(),
    );
    assert_eq!(keys(&b, 1), ["ACME-1"]);
    let card = &b.columns[1].tasks[0];
    let subs: Vec<_> = card
        .subtasks
        .iter()
        .map(|s| (s.issue.as_ref().unwrap().key.as_str(), s.done))
        .collect();
    assert_eq!(subs, [("ACME-2", false), ("ACME-3", true)]);
    assert_eq!(keys(&b, 0), ["ACME-5"], "an orphan shows as a card");
}

#[test]
fn epics_are_cards_and_name_their_stories() {
    let mut b = jira_board();
    let mut epic = issue(9, TODO, "0|z");
    epic.kind = TaskKind::Epic;
    epic.title = "Billing".into();
    let mut child = issue(1, DOING, "0|a");
    child.issue.epic = Some("ACME-9".into());
    child.epic_summary = Some("Billing (old name)".into());
    b.merge_issues(vec![child.clone(), epic.clone()], true, ids());
    assert_eq!(
        keys(&b, 0),
        ["ACME-9"],
        "the epic is a card in its status's column"
    );
    assert_eq!(keys(&b, 1), ["ACME-1"], "its story stays in its own column");
    assert_eq!(b.columns[0].tasks[0].kind, Some(TaskKind::Epic));
    let e = b.epic("ACME-9").unwrap();
    assert_eq!(e.summary, "Billing");
    assert_eq!(e.color, crate::board::tag_color("ACME-9"));

    // An epic no story names stays while it is a card…
    child.issue.epic = None;
    b.merge_issues(vec![child.clone()], false, ids());
    assert!(b.epic("ACME-9").is_some());
    // …and is forgotten with it.
    b.merge_issues(vec![child], true, ids());
    assert!(keys(&b, 0).is_empty());
    assert!(b.epics.is_empty());
}

#[test]
fn an_epic_named_only_by_its_stories_is_still_named() {
    let mut b = jira_board();
    let mut child = issue(1, TODO, "0|a");
    child.issue.epic = Some("ACME-9".into());
    child.epic_summary = Some("Billing".into());
    b.merge_issues(vec![child], true, ids());
    assert_eq!(keys(&b, 0), ["ACME-1"]);
    assert_eq!(b.epic("ACME-9").unwrap().summary, "Billing");
}

#[test]
fn long_descriptions_are_cut_and_become_read_only() {
    let mut b = jira_board();
    let mut long = issue(1, TODO, "0|a");
    long.notes = "x".repeat(MAX_NOTES + 5);
    let mut short = issue(2, TODO, "0|b");
    short.notes = "fine\n\n".into();
    b.merge_issues(vec![long, short], true, ids());
    let t = b.task_for_issue("ACME-1").unwrap();
    assert_eq!(t.notes.chars().count(), MAX_NOTES);
    assert!(!t.issue.as_ref().unwrap().notes_editable);
    let t = b.task_for_issue("ACME-2").unwrap();
    assert_eq!(t.notes, "fine");
    assert!(t.issue.as_ref().unwrap().notes_editable);
}

#[test]
fn a_board_keeps_at_most_max_synced_issues() {
    let mut b = jira_board();
    let snapshot: Vec<_> = (0..u32::try_from(MAX_SYNCED_ISSUES).unwrap() + 3)
        .map(|n| issue(n, TODO, &format!("0|{n:06}")))
        .collect();
    let report = b.merge_issues(snapshot, true, ids());
    assert_eq!(b.all_tasks().count(), MAX_SYNCED_ISSUES);
    assert_eq!(report.dropped, 3);
}

#[test]
fn columns_with_the_same_name_keep_their_id_and_color() {
    let mut b = jira_board();
    b.merge_issues(vec![issue(1, DONE, "0|a")], true, ids());
    let done = b.columns[2].id;
    b.update_column(done, None, Some(Some(LabelColor::Green)))
        .unwrap();
    b.set_jira_columns(
        &[column("Backlog", &[TODO]), column("Done", &[DONE, DOING])],
        || ColumnId::from_u128(99),
    )
    .unwrap();
    assert_eq!(b.columns[1].id, done);
    assert_eq!(b.columns[1].color, Some(LabelColor::Green));
    b.merge_issues(Vec::new(), false, ids());
    assert_eq!(keys(&b, 1), ["ACME-1"]);
    assert!(b.set_jira_columns(&[], || ColumnId::from_u128(98)).is_err());
}

#[test]
fn changes_jira_owns_are_refused_and_change_nothing() {
    let mut b = jira_board();
    b.merge_issues(
        vec![issue(1, TODO, "0|a"), issue(2, TODO, "0|b")],
        true,
        ids(),
    );
    let one = b.task_for_issue("ACME-1").unwrap().id;
    let two = b.task_for_issue("ACME-2").unwrap().id;
    let column = b.columns[1].id;
    let before = b.clone();
    let follows = |r: Result<_, BoardError>| matches!(r, Err(BoardError::FollowsJira(_)));
    assert!(follows(b.rename("x")));
    assert!(follows(b.move_task(one, column, 0)));
    assert!(follows(b.nest_task(one, two, 0)));
    assert!(follows(b.remove_task(one).map(|_| ())));
    assert!(follows(b.add_column(ColumnId::from_u128(50), "x", 0)));
    assert!(follows(b.update_column(column, Some("x"), None)));
    assert!(follows(b.remove_column(column).map(|_| ())));
    assert!(follows(b.update_task(
        one,
        &TaskPatch {
            title: Some("x".into()),
            ..TaskPatch::default()
        }
    )));
    assert!(follows(b.update_task(
        one,
        &TaskPatch {
            pr_url: Some(PrUrl::parse("https://h/pull/1").ok()),
            ..TaskPatch::default()
        }
    )));
    assert_eq!(b, before);
}

#[test]
fn a_jira_move_shows_at_once_and_can_be_put_back() {
    let mut b = jira_board();
    b.merge_issues(
        vec![
            issue(1, TODO, "0|a"),
            issue(2, TODO, "0|b"),
            issue(3, DOING, "0|c"),
        ],
        true,
        ids(),
    );
    let card = b.columns[0].tasks[0].id;
    let doing = b.columns[1].id;
    let from = b.show_jira_move(card, doing, 0).unwrap();
    assert_eq!(
        (keys(&b, 0), keys(&b, 1)),
        (vec!["ACME-2"], vec!["ACME-1", "ACME-3"])
    );
    b.undo_jira_move(card, from).unwrap();
    assert_eq!(
        (keys(&b, 0), keys(&b, 1)),
        (vec!["ACME-1", "ACME-2"], vec!["ACME-3"])
    );
    // Only on a board that follows Jira, and only to a column it has.
    assert!(b.show_jira_move(card, ColumnId::from_u128(999), 0).is_err());
    assert_eq!(
        keys(&b, 0),
        ["ACME-1", "ACME-2"],
        "a refused move changes nothing"
    );
    let mut own = Board::new(
        BoardId::from_u128(2),
        "own",
        Timestamp::from_unix_millis(1),
        [3, 4, 5].map(ColumnId::from_u128),
    )
    .unwrap();
    assert!(own.show_jira_move(card, ColumnId::from_u128(3), 0).is_err());
}

#[test]
fn pull_requests_are_read_for_linked_issues_and_kept_by_syncs() {
    let mut b = jira_board();
    b.merge_issues(
        vec![
            issue(1, TODO, "0|a"),
            issue(2, TODO, "0|b"),
            subtask(3, 2, TODO),
        ],
        true,
        ids(),
    );
    assert!(b.linked_issue_ids().is_empty());
    let sub = b.task_for_issue("ACME-3").unwrap().id;
    b.link_thread(sub, ThreadId::from_u128(9)).unwrap();
    assert_eq!(b.linked_issue_ids(), ["10003"]);

    let pr = PullRequest::new(
        PrUrl::parse("https://github.com/o/r/pull/7").unwrap(),
        "ACME-3 fix",
        PrStatus::Open,
    );
    assert!(b.set_pull_requests("10003", vec![pr.clone()]));
    assert!(
        !b.set_pull_requests("10003", vec![pr.clone()]),
        "same again"
    );
    assert!(
        !b.set_pull_requests("99999", vec![pr.clone()]),
        "not on the board"
    );

    // A sync rewrites the issue from Jira's fields but keeps what dev info said.
    let mut renamed = subtask(3, 2, DOING);
    renamed.title = "Renamed".into();
    let report = b.merge_issues(vec![issue(2, TODO, "0|b"), renamed], false, ids());
    assert!(report.changed);
    let issue = b.task_for_issue("ACME-3").unwrap().issue.as_ref().unwrap();
    assert_eq!(issue.pull_requests, [pr]);

    let many = (0..20)
        .map(|n| {
            PullRequest::new(
                PrUrl::parse(&format!("https://h/pull/{n}")).unwrap(),
                "",
                PrStatus::Merged,
            )
        })
        .collect();
    assert!(b.set_pull_requests("10003", many));
    let issue = b.task_for_issue("ACME-3").unwrap().issue.as_ref().unwrap();
    assert_eq!(issue.pull_requests.len(), crate::board::MAX_PULL_REQUESTS);
}
