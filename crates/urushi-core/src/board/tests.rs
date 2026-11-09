//! Unit tests for boards, tasks and subtasks.

use super::*;

fn board() -> Board {
    Board::new(
        BoardId::from_u128(1),
        "  Launch   plan ",
        Timestamp::from_unix_millis(5),
        [10, 11, 12].map(ColumnId::from_u128),
    )
    .unwrap()
}

fn task(n: u128, title: &str) -> Task {
    Task::new(
        TaskId::from_u128(n),
        title,
        "",
        Timestamp::from_unix_millis(7),
    )
    .unwrap()
}

fn titles(board: &Board, column: usize) -> Vec<&str> {
    board.columns[column]
        .tasks
        .iter()
        .map(|t| t.title.as_str())
        .collect()
}

#[test]
fn a_new_board_has_the_default_columns_and_a_clean_name() {
    let b = board();
    assert_eq!(b.name, "Launch plan");
    let names: Vec<_> = b.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, DEFAULT_COLUMNS);
    assert_eq!(b.task_count(), 0);
}

#[test]
fn names_titles_and_notes_are_validated() {
    assert_eq!(board_name(" \t"), Err(BoardError::EmptyName));
    assert!(matches!(
        board_name(&"x".repeat(MAX_NAME + 1)),
        Err(BoardError::TooLong { .. })
    ));
    assert_eq!(task_title(" fix \n the  bug "), Ok("fix the bug".into()));
    assert_eq!(task_title("  "), Err(BoardError::EmptyTitle));
    assert_eq!(
        task_notes("line 1\nline 2\n\n"),
        Ok("line 1\nline 2".into())
    );
    assert!(task_notes(&"x".repeat(MAX_NOTES + 1)).is_err());
}

#[test]
fn tasks_move_within_and_between_columns() {
    let mut b = board();
    let todo = b.columns[0].id;
    let done = b.columns[2].id;
    for (n, t) in [(1, "a"), (2, "b"), (3, "c")] {
        b.add_task(todo, task(n, t)).unwrap();
    }
    // To the bottom of its own column.
    b.move_task(TaskId::from_u128(1), todo, 99).unwrap();
    assert_eq!(titles(&b, 0), ["b", "c", "a"]);
    // To the top.
    b.move_task(TaskId::from_u128(1), todo, 0).unwrap();
    assert_eq!(titles(&b, 0), ["a", "b", "c"]);
    // To another column.
    b.move_task(TaskId::from_u128(2), done, 0).unwrap();
    assert_eq!(titles(&b, 0), ["a", "c"]);
    assert_eq!(titles(&b, 2), ["b"]);
    assert_eq!(b.task_count(), 3);
}

#[test]
fn failed_operations_change_nothing() {
    let mut b = board();
    let todo = b.columns[0].id;
    b.add_task(todo, task(1, "a")).unwrap();
    let before = b.clone();
    let nowhere = ColumnId::from_u128(99);
    assert_eq!(
        b.move_task(TaskId::from_u128(1), nowhere, 0),
        Err(BoardError::NoSuchColumn(nowhere))
    );
    assert!(b.add_task(nowhere, task(2, "b")).is_err());
    assert!(b.remove_task(TaskId::from_u128(2)).is_err());
    assert_eq!(
        b.update_task(
            TaskId::from_u128(1),
            Some("ok"),
            Some(&"x".repeat(MAX_NOTES + 1))
        ),
        Err(BoardError::TooLong {
            what: "task notes",
            max: MAX_NOTES
        })
    );
    let missing = SubtaskId::from_u128(77);
    assert_eq!(
        b.update_subtask(missing, Some("x"), None),
        Err(BoardError::NoSuchSubtask(missing))
    );
    assert!(b.remove_subtask(missing).is_err());
    assert_eq!(
        b.add_subtask(TaskId::from_u128(9), Subtask::new(missing, "x").unwrap()),
        Err(BoardError::NoSuchTask(TaskId::from_u128(9)))
    );
    assert_eq!(b, before);
}

#[test]
fn updating_a_task_keeps_unset_fields() {
    let mut b = board();
    let todo = b.columns[0].id;
    b.add_task(todo, task(1, "a")).unwrap();
    let id = TaskId::from_u128(1);
    b.update_task(id, None, Some("details")).unwrap();
    b.update_task(id, Some("renamed"), None).unwrap();
    let t = b.task(id).unwrap();
    assert_eq!((t.title.as_str(), t.notes.as_str()), ("renamed", "details"));
}

#[test]
fn subtasks_are_added_checked_renamed_and_removed() {
    let mut b = board();
    let todo = b.columns[0].id;
    b.add_task(todo, task(1, "a")).unwrap();
    let parent = TaskId::from_u128(1);
    let (s1, s2) = (SubtaskId::from_u128(21), SubtaskId::from_u128(22));
    assert_eq!(Subtask::new(s1, "  "), Err(BoardError::EmptyTitle));
    b.add_subtask(parent, Subtask::new(s1, "write  test").unwrap())
        .unwrap();
    b.add_subtask(parent, Subtask::new(s2, "fix").unwrap())
        .unwrap();
    b.update_subtask(s1, None, Some(true)).unwrap();
    b.update_subtask(s2, Some("fix it"), None).unwrap();
    let t = b.task(parent).unwrap();
    let shown: Vec<_> = t
        .subtasks
        .iter()
        .map(|s| (s.title.as_str(), s.done))
        .collect();
    assert_eq!(shown, [("write test", true), ("fix it", false)]);
    assert_eq!(t.subtasks_done(), (1, 2));
    assert_eq!(b.subtask(s2).map(|(t, _)| t.id), Some(parent));
    assert_eq!(b.remove_subtask(s1).unwrap().id, s1);
    assert_eq!(b.task(parent).unwrap().subtasks.len(), 1);
    // Moving the task keeps its subtasks.
    b.move_task(parent, b.columns[2].id, 0).unwrap();
    assert_eq!(b.task(parent).unwrap().subtasks.len(), 1);
}

#[test]
fn a_task_holds_a_bounded_number_of_subtasks() {
    let mut b = board();
    let todo = b.columns[0].id;
    b.add_task(todo, task(1, "a")).unwrap();
    for n in 0..MAX_SUBTASKS {
        let id = SubtaskId::from_u128(1000 + n as u128);
        b.add_subtask(TaskId::from_u128(1), Subtask::new(id, "s").unwrap())
            .unwrap();
    }
    let extra = Subtask::new(SubtaskId::from_u128(5), "one more").unwrap();
    assert_eq!(
        b.add_subtask(TaskId::from_u128(1), extra),
        Err(BoardError::TooManySubtasks)
    );
}

#[test]
fn threads_are_linked_and_unlinked() {
    let mut b = board();
    let todo = b.columns[0].id;
    b.add_task(todo, task(1, "a")).unwrap();
    let (parent, sub) = (TaskId::from_u128(1), SubtaskId::from_u128(2));
    b.add_subtask(parent, Subtask::new(sub, "s").unwrap())
        .unwrap();
    let (t1, t2) = (ThreadId::from_u128(8), ThreadId::from_u128(9));
    b.link_thread(parent, None, t1).unwrap();
    b.link_thread(parent, Some(sub), t2).unwrap();
    let task = b.task(parent).unwrap();
    assert_eq!((task.thread, task.subtasks[0].thread), (Some(t1), Some(t2)));
    assert_eq!(
        b.link_thread(parent, Some(SubtaskId::from_u128(3)), t1),
        Err(BoardError::NoSuchSubtask(SubtaskId::from_u128(3)))
    );
    assert!(b.unlink_thread(t2));
    assert!(!b.unlink_thread(t2), "nothing left to unlink");
    let task = b.task(parent).unwrap();
    assert_eq!((task.thread, task.subtasks[0].thread), (Some(t1), None));
}
