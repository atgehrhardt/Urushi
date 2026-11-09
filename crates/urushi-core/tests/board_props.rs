//! Property tests: no sequence of board operations may lose or duplicate a task.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use std::collections::BTreeSet;

use proptest::prelude::*;
use urushi_core::{Board, BoardId, ColumnId, Task, TaskId, Timestamp};

#[derive(Clone, Debug)]
enum Op {
    Add {
        column: usize,
    },
    Move {
        task: usize,
        column: usize,
        index: usize,
    },
    Remove {
        task: usize,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..4usize).prop_map(|column| Op::Add { column }),
        (0..40usize, 0..4usize, 0..12usize).prop_map(|(task, column, index)| Op::Move {
            task,
            column,
            index
        }),
        (0..40usize).prop_map(|task| Op::Remove { task }),
    ]
}

fn ids(board: &Board) -> Vec<TaskId> {
    board
        .columns
        .iter()
        .flat_map(|c| c.tasks.iter().map(|t| t.id))
        .collect()
}

proptest! {
    #[test]
    fn tasks_are_never_lost_or_duplicated(ops in prop::collection::vec(op(), 0..60)) {
        let mut board = Board::new(
            BoardId::from_u128(1),
            "b",
            Timestamp::from_unix_millis(0),
            [10, 11, 12].map(ColumnId::from_u128),
        )
        .unwrap();
        // Column index 3 doesn't exist: those operations must fail and change nothing.
        let column = |i: usize| board_column(i);
        let mut expected: BTreeSet<TaskId> = BTreeSet::new();
        let mut next = 100u128;
        for op in ops {
            let before = board.clone();
            match op {
                Op::Add { column: c } => {
                    let task = Task::new(TaskId::from_u128(next), "t", "", Timestamp::from_unix_millis(0)).unwrap();
                    if board.add_task(column(c), task).is_ok() {
                        expected.insert(TaskId::from_u128(next));
                    } else {
                        prop_assert_eq!(&board, &before);
                    }
                    next += 1;
                }
                Op::Move { task, column: c, index } => {
                    let all = ids(&board);
                    let id = all.get(task).copied().unwrap_or(TaskId::from_u128(9999));
                    if board.move_task(id, column(c), index).is_ok() {
                        let tasks = &board.columns[c].tasks;
                        let at = tasks.iter().position(|t| t.id == id).unwrap();
                        prop_assert_eq!(at, index.min(tasks.len() - 1));
                    } else {
                        prop_assert_eq!(&board, &before);
                    }
                }
                Op::Remove { task } => {
                    let all = ids(&board);
                    if let Some(id) = all.get(task).copied() {
                        board.remove_task(id).unwrap();
                        expected.remove(&id);
                    }
                }
            }
            let now = ids(&board);
            let unique: BTreeSet<TaskId> = now.iter().copied().collect();
            prop_assert_eq!(unique.len(), now.len(), "a task appears twice");
            prop_assert_eq!(&unique, &expected);
            prop_assert_eq!(board.columns.len(), 3);
        }
    }
}

fn board_column(i: usize) -> ColumnId {
    ColumnId::from_u128(10 + i as u128)
}

#[derive(Clone, Debug)]
enum ColumnOp {
    AddTask { column: usize },
    AddColumn { index: usize },
    MoveColumn { column: usize, index: usize },
    RemoveColumn { column: usize },
}

fn column_op() -> impl Strategy<Value = ColumnOp> {
    prop_oneof![
        (0..6usize).prop_map(|column| ColumnOp::AddTask { column }),
        (0..6usize).prop_map(|index| ColumnOp::AddColumn { index }),
        (0..6usize, 0..6usize).prop_map(|(column, index)| ColumnOp::MoveColumn { column, index }),
        (0..6usize).prop_map(|column| ColumnOp::RemoveColumn { column }),
    ]
}

proptest! {
    #[test]
    fn column_changes_never_lose_tasks_or_the_last_column(
        ops in prop::collection::vec(column_op(), 0..60)
    ) {
        let mut board = Board::new(
            BoardId::from_u128(1),
            "b",
            Timestamp::from_unix_millis(0),
            [10, 11, 12].map(ColumnId::from_u128),
        )
        .unwrap();
        let mut expected: BTreeSet<TaskId> = BTreeSet::new();
        let mut next = 100u128;
        for op in ops {
            let at = |i: usize| board.columns.get(i).map_or(ColumnId::from_u128(9999), |c| c.id);
            let before = board.clone();
            let ok = match op {
                ColumnOp::AddTask { column } => {
                    let task = Task::new(TaskId::from_u128(next), "t", "", Timestamp::from_unix_millis(0)).unwrap();
                    let ok = board.add_task(at(column), task).is_ok();
                    if ok {
                        expected.insert(TaskId::from_u128(next));
                    }
                    ok
                }
                ColumnOp::AddColumn { index } => board.add_column(ColumnId::from_u128(next), "c", index).is_ok(),
                ColumnOp::MoveColumn { column, index } => board.move_column(at(column), index).is_ok(),
                ColumnOp::RemoveColumn { column } => board.remove_column(at(column)).is_ok(),
            };
            next += 1;
            if !ok {
                prop_assert_eq!(&board, &before);
            }
            prop_assert!(!board.columns.is_empty());
            let now = ids(&board);
            let unique: BTreeSet<TaskId> = now.iter().copied().collect();
            prop_assert_eq!(unique.len(), now.len(), "a task appears twice");
            prop_assert_eq!(&unique, &expected);
        }
    }
}
