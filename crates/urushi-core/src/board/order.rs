//! The order of boards. The list of boards is kept in the order their tabs are shown,
//! manual and Jira boards alike; reordering it is Urushi's own and never reaches Jira.

use super::Board;
use crate::BoardId;

/// Moves board `id` to `index` in the list (counted without it; clamped to the end).
/// Returns `false`, changing nothing, if there is no such board.
pub fn move_board(boards: &mut Vec<Board>, id: BoardId, index: usize) -> bool {
    let Some(at) = boards.iter().position(|b| b.id == id) else {
        return false;
    };
    let board = boards.remove(at);
    boards.insert(index.min(boards.len()), board);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnId, Timestamp};

    fn boards(n: u128) -> Vec<Board> {
        (0..n)
            .map(|i| {
                Board::new(
                    BoardId::from_u128(i),
                    &format!("b{i}"),
                    Timestamp::unknown(),
                    std::array::from_fn(|c| ColumnId::from_u128(i * 10 + c as u128)),
                )
                .unwrap()
            })
            .collect()
    }

    fn order(boards: &[Board]) -> Vec<&str> {
        boards.iter().map(|b| b.name.as_str()).collect()
    }

    #[test]
    fn boards_move_left_and_right() {
        let mut list = boards(3);
        assert!(move_board(&mut list, BoardId::from_u128(2), 0));
        assert_eq!(order(&list), ["b2", "b0", "b1"]);
        assert!(move_board(&mut list, BoardId::from_u128(2), 2));
        assert_eq!(order(&list), ["b0", "b1", "b2"]);
        assert!(move_board(&mut list, BoardId::from_u128(0), 1));
        assert_eq!(order(&list), ["b1", "b0", "b2"]);
    }

    #[test]
    fn moving_clamps_to_the_end_and_keeps_its_place() {
        let mut list = boards(3);
        assert!(move_board(&mut list, BoardId::from_u128(0), 99));
        assert_eq!(order(&list), ["b1", "b2", "b0"]);
        assert!(move_board(&mut list, BoardId::from_u128(2), 1));
        assert_eq!(order(&list), ["b1", "b2", "b0"]);
    }

    #[test]
    fn an_unknown_board_changes_nothing() {
        let mut list = boards(2);
        assert!(!move_board(&mut list, BoardId::from_u128(7), 0));
        assert_eq!(order(&list), ["b0", "b1"]);
    }
}
