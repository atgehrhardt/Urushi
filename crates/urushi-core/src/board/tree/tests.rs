//! Subtasks: nested tasks, moving them onto the board and under other tasks, the
//! bounds, and thread links.

use super::super::*;
use crate::{BoardId, ColumnId, TaskId, ThreadId, Timestamp};

fn board() -> Board {
    Board::new(
        BoardId::from_u128(1),
        "b",
        Timestamp::from_unix_millis(0),
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

fn id(n: u128) -> TaskId {
    TaskId::from_u128(n)
}

const TODO: u128 = 10;
const DONE: u128 = 12;

fn col(n: u128) -> ColumnId {
    ColumnId::from_u128(n)
}

#[test]
fn subtasks_are_tasks_found_at_any_depth() {
    let mut b = board();
    b.add_task(col(TODO), task(1, "card")).unwrap();
    b.add_subtask(id(1), task(2, "step")).unwrap();
    b.add_subtask(id(2), task(3, "smaller step")).unwrap();
    assert_eq!(b.task(id(3)).unwrap().title, "smaller step");
    let path: Vec<_> = b.path_to(id(3)).unwrap().iter().map(|t| t.id).collect();
    assert_eq!(path, [id(1), id(2), id(3)]);
    assert_eq!(b.column_of(id(3)), Some(col(TODO)));
    assert_eq!(b.task_count(), 1, "cards only");
    assert_eq!(b.all_tasks().count(), 3);
    // Checking off and editing works at any depth.
    b.update_task(
        id(3),
        &TaskPatch {
            done: Some(true),
            title: Some("renamed".into()),
            ..TaskPatch::default()
        },
    )
    .unwrap();
    assert!(b.task(id(3)).unwrap().done);
    assert_eq!(b.task(id(2)).unwrap().subtasks_done(), (1, 1));
    // Removing a task takes its subtasks with it.
    let removed = b.remove_task(id(2)).unwrap();
    assert_eq!(removed.subtree().len(), 2);
    assert!(b.task(id(3)).is_none());
}

#[test]
fn a_subtask_moves_onto_the_board_and_a_card_under_a_task() {
    let mut b = board();
    b.add_task(col(TODO), task(1, "card")).unwrap();
    b.add_task(col(DONE), task(2, "finished")).unwrap();
    let mut step = task(3, "step");
    step.done = true;
    step.subtasks.push(task(4, "inner"));
    b.add_subtask(id(1), step).unwrap();

    // Split out: a card of its own, unchecked, with its subtasks.
    b.move_task(id(3), col(TODO), 0).unwrap();
    assert_eq!(b.columns[0].tasks[0].id, id(3));
    assert!(!b.columns[0].tasks[0].done);
    assert_eq!(b.columns[0].tasks[0].subtasks[0].id, id(4));
    assert!(b.task(id(1)).unwrap().subtasks.is_empty());

    // Nest a card from the last column: it arrives checked off.
    b.nest_task(id(2), id(1), 0).unwrap();
    assert!(b.task(id(2)).unwrap().done);
    assert!(b.columns[2].tasks.is_empty());
    // And one from another column arrives unchecked, at the index asked.
    b.nest_task(id(3), id(1), 0).unwrap();
    let order: Vec<_> = b
        .task(id(1))
        .unwrap()
        .subtasks
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(order, [id(3), id(2)]);
    assert!(!b.task(id(3)).unwrap().done);
    // Reordering under the same parent keeps the check marks.
    b.nest_task(id(2), id(1), 0).unwrap();
    let order: Vec<_> = b
        .task(id(1))
        .unwrap()
        .subtasks
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(order, [id(2), id(3)]);
    assert!(b.task(id(2)).unwrap().done);
}

#[test]
fn a_task_never_goes_under_itself() {
    let mut b = board();
    b.add_task(col(TODO), task(1, "card")).unwrap();
    b.add_subtask(id(1), task(2, "step")).unwrap();
    b.add_subtask(id(2), task(3, "inner")).unwrap();
    let before = b.clone();
    assert_eq!(b.nest_task(id(1), id(3), 0), Err(BoardError::Cycle));
    assert_eq!(b.nest_task(id(2), id(2), 0), Err(BoardError::Cycle));
    assert_eq!(b, before);
}

#[test]
fn nesting_is_bounded_in_depth_and_breadth() {
    let mut b = board();
    b.add_task(col(TODO), task(1, "card")).unwrap();
    // A chain as deep as allowed.
    for level in 0..MAX_DEPTH {
        let n = 2 + level as u128;
        b.add_subtask(id(n - 1), task(n, "step")).unwrap();
    }
    let deepest = id(1 + MAX_DEPTH as u128);
    assert_eq!(
        b.add_subtask(deepest, task(99, "too deep")),
        Err(BoardError::TooDeep)
    );
    // A card with a subtask can't go under the deepest step either.
    b.add_task(col(TODO), task(50, "other")).unwrap();
    b.add_subtask(id(50), task(51, "its step")).unwrap();
    let before = b.clone();
    assert_eq!(
        b.nest_task(id(50), id(MAX_DEPTH as u128), 0),
        Err(BoardError::TooDeep)
    );
    assert_eq!(b, before);

    let mut b = board();
    b.add_task(col(TODO), task(1, "card")).unwrap();
    for n in 0..MAX_SUBTASKS {
        b.add_subtask(id(1), task(1000 + n as u128, "s")).unwrap();
    }
    assert_eq!(
        b.add_subtask(id(1), task(5, "one more")),
        Err(BoardError::TooManySubtasks)
    );
    b.add_task(col(TODO), task(6, "card")).unwrap();
    assert_eq!(
        b.nest_task(id(6), id(1), 0),
        Err(BoardError::TooManySubtasks)
    );
}

#[test]
fn threads_are_linked_and_unlinked_at_any_depth() {
    let mut b = board();
    b.add_task(col(TODO), task(1, "card")).unwrap();
    b.add_subtask(id(1), task(2, "step")).unwrap();
    b.add_subtask(id(2), task(3, "inner")).unwrap();
    let (t1, t2) = (ThreadId::from_u128(8), ThreadId::from_u128(9));
    b.link_thread(id(1), t1).unwrap();
    b.link_thread(id(3), t2).unwrap();
    assert_eq!(b.task(id(3)).unwrap().thread, Some(t2));
    assert_eq!(b.link_thread(id(7), t1), Err(BoardError::NoSuchTask(id(7))));
    assert!(b.unlink_thread(t2));
    assert!(!b.unlink_thread(t2), "nothing left to unlink");
    assert_eq!(
        (b.task(id(1)).unwrap().thread, b.task(id(3)).unwrap().thread),
        (Some(t1), None)
    );
}

#[test]
fn old_checklist_subtasks_load_as_tasks() {
    let json = r#"{"id":"00000000-0000-0000-0000-000000000001","title":"card","notes":"",
        "created_at":5,"subtasks":[{"id":"00000000-0000-0000-0000-000000000002",
        "title":"step","done":true,"thread":null}]}"#;
    let t: Task = serde_json::from_str(json).unwrap();
    let step = &t.subtasks[0];
    assert_eq!((step.title.as_str(), step.done), ("step", true));
    assert!(step.subtasks.is_empty() && step.tags.is_empty() && step.color.is_none());
    assert_eq!(step.created_at, Timestamp::unknown());
}
