//! Unit tests for boards, columns and tasks (subtasks are in `tree/tests.rs`).

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
            &TaskPatch {
                title: Some("ok".into()),
                notes: Some("x".repeat(MAX_NOTES + 1)),
                ..TaskPatch::default()
            }
        ),
        Err(BoardError::TooLong {
            what: "task notes",
            max: MAX_NOTES
        })
    );
    let missing = TaskId::from_u128(77);
    assert_eq!(
        b.add_subtask(TaskId::from_u128(9), task(77, "x")),
        Err(BoardError::NoSuchTask(TaskId::from_u128(9)))
    );
    assert_eq!(
        b.nest_task(missing, TaskId::from_u128(1), 0),
        Err(BoardError::NoSuchTask(missing))
    );
    assert_eq!(
        b.nest_task(TaskId::from_u128(1), TaskId::from_u128(1), 0),
        Err(BoardError::Cycle)
    );
    assert_eq!(b, before);
}

#[test]
fn updating_a_task_keeps_unset_fields() {
    let mut b = board();
    let todo = b.columns[0].id;
    b.add_task(todo, task(1, "a")).unwrap();
    let id = TaskId::from_u128(1);
    let patch = |p: TaskPatch| p;
    b.update_task(
        id,
        &patch(TaskPatch {
            notes: Some("details".into()),
            color: Some(Some(LabelColor::Teal)),
            tags: Some(vec!["ui".into(), " UI ".into(), "docs".into()]),
            ..TaskPatch::default()
        }),
    )
    .unwrap();
    b.update_task(
        id,
        &TaskPatch {
            title: Some("renamed".into()),
            ..TaskPatch::default()
        },
    )
    .unwrap();
    let t = b.task(id).unwrap();
    assert_eq!((t.title.as_str(), t.notes.as_str()), ("renamed", "details"));
    assert_eq!(
        (t.color, t.tags.as_slice()),
        (
            Some(LabelColor::Teal),
            &["ui".to_owned(), "docs".to_owned()][..]
        )
    );
    // Clearing the color; bad tags change nothing.
    b.update_task(
        id,
        &TaskPatch {
            color: Some(None),
            ..TaskPatch::default()
        },
    )
    .unwrap();
    let before = b.clone();
    let many = (0..=MAX_TAGS).map(|i| i.to_string()).collect();
    assert_eq!(
        b.update_task(
            id,
            &TaskPatch {
                title: Some("x".into()),
                tags: Some(many),
                ..TaskPatch::default()
            }
        ),
        Err(BoardError::TooManyTags)
    );
    assert_eq!(b, before);
    assert_eq!(b.task(id).unwrap().color, None);
}

#[test]
fn columns_are_added_renamed_colored_moved_and_removed() {
    let mut b = board();
    let [todo, doing, done] = [0, 1, 2].map(|i| b.columns[i].id);
    let review = ColumnId::from_u128(20);
    b.add_column(review, " Review ", 2).unwrap();
    assert_eq!(b.columns[2].name, "Review");
    assert_eq!(
        b.add_column(ColumnId::from_u128(21), "  ", 0),
        Err(BoardError::EmptyColumnName)
    );
    b.update_column(review, Some("In review"), Some(Some(LabelColor::Purple)))
        .unwrap();
    assert_eq!(b.column(review).unwrap().color, Some(LabelColor::Purple));
    b.update_column(review, None, Some(None)).unwrap();
    assert_eq!(b.column(review).unwrap().color, None);
    b.move_column(review, 0).unwrap();
    let order: Vec<_> = b.columns.iter().map(|c| c.id).collect();
    assert_eq!(order, [review, todo, doing, done]);
    b.move_column(review, 99).unwrap();
    assert_eq!(b.columns.last().unwrap().id, review);

    // Removing moves the tasks left, or right for the first column.
    b.add_task(doing, task(1, "a")).unwrap();
    b.add_task(todo, task(2, "b")).unwrap();
    b.add_task(doing, task(3, "c")).unwrap();
    assert_eq!(b.column_heir(doing), Ok(Some(todo)));
    assert_eq!(b.remove_column(doing), Ok(todo));
    assert_eq!(titles(&b, 0), ["b", "a", "c"]);
    assert_eq!(b.column_heir(todo), Ok(Some(done)));
    assert_eq!(b.remove_column(todo), Ok(done));
    assert_eq!(b.columns[0].id, done);
    assert_eq!(titles(&b, 0), ["b", "a", "c"]);
    b.remove_column(review).unwrap();
    let before = b.clone();
    assert_eq!(b.remove_column(done), Err(BoardError::LastColumn));
    assert_eq!(b, before);
    assert_eq!(b.task_count(), 3);
}

#[test]
fn a_board_holds_a_bounded_number_of_columns() {
    let mut b = board();
    for i in 3..MAX_COLUMNS {
        b.add_column(ColumnId::from_u128(100 + i as u128), "c", 99)
            .unwrap();
    }
    assert_eq!(
        b.add_column(ColumnId::from_u128(999), "c", 0),
        Err(BoardError::TooManyColumns)
    );
}
