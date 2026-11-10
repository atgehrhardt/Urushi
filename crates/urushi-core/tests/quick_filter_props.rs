//! Property tests: no sequence of quick-filter operations loses, duplicates or reorders
//! a board's quick filters, keeps one beyond the bound, or changes anything when it
//! fails.
#![allow(clippy::unwrap_used, reason = "test code")]

use proptest::prelude::*;
use urushi_core::{
    Board, BoardId, ColumnId, QuickFilterId, QuickRule, TaskKind, Timestamp,
    board::MAX_QUICK_FILTERS,
};

#[derive(Clone, Debug)]
enum Op {
    Add { name: String, empty: bool },
    Rename { filter: u128, name: String },
    Remove { filter: u128 },
}

fn op() -> impl Strategy<Value = Op> {
    let name = prop_oneof![Just(String::new()), Just(" ".to_owned()), "[a-z ]{1,12}"];
    prop_oneof![
        (name.clone(), any::<bool>()).prop_map(|(name, empty)| Op::Add { name, empty }),
        (0..30u128, name).prop_map(|(filter, name)| Op::Rename { filter, name }),
        (0..30u128).prop_map(|filter| Op::Remove { filter }),
    ]
}

fn rule(empty: bool) -> QuickRule {
    QuickRule::Fields {
        search: String::new(),
        kinds: if empty {
            Vec::new()
        } else {
            vec![TaskKind::Bug]
        },
        tags: Vec::new(),
    }
}

proptest! {
    #[test]
    fn quick_filters_are_never_lost_duplicated_or_reordered(
        ops in prop::collection::vec(op(), 0..60)
    ) {
        let mut board = Board::new(
            BoardId::from_u128(1),
            "b",
            Timestamp::from_unix_millis(0),
            [10, 11, 12].map(ColumnId::from_u128),
        )
        .unwrap();
        let mut expected: Vec<u128> = Vec::new();
        let mut next = 0u128;
        for op in ops {
            let before = board.clone();
            let result = match op {
                Op::Add { name, empty } => {
                    let id = next;
                    next += 1;
                    let r = board.add_quick_filter(QuickFilterId::from_u128(id), &name, rule(empty));
                    if r.is_ok() {
                        expected.push(id);
                    }
                    r
                }
                Op::Rename { filter, name } => {
                    board.update_quick_filter(QuickFilterId::from_u128(filter), Some(&name), None)
                }
                Op::Remove { filter } => {
                    let r = board.remove_quick_filter(QuickFilterId::from_u128(filter)).map(drop);
                    if r.is_ok() {
                        expected.retain(|e| *e != filter);
                    }
                    r
                }
            };
            if result.is_err() {
                prop_assert_eq!(&board, &before);
            }
            let ids: Vec<u128> = board.quick_filters.iter().map(|q| q.id.as_uuid().as_u128()).collect();
            prop_assert_eq!(&ids, &expected);
            prop_assert!(ids.len() <= MAX_QUICK_FILTERS);
            prop_assert!(board.quick_filters.iter().all(|q| !q.name.trim().is_empty()));
        }
    }
}
