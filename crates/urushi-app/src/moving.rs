//! Moving a card without waiting for the daemon (ADR 0056, UR-71).
//!
//! A dropped card shows at once where it was dropped: the board on screen is the
//! daemon's with the card taken out and put back at the drop's place, the same edit the
//! daemon makes (`MoveTask` counts positions without the moved card), so nothing is
//! decided here. A Jira move waits for Jira, and syncs of the old picture may arrive
//! meanwhile; the card stays put through them. The preview ends with the daemon's
//! answer: by then its workspace shows the move, or, if it refused, the card goes back.

use urushi_client::v1;

/// A card moved on screen ahead of the daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardMove {
    /// The number of the move, matching its answer.
    pub query: u64,
    /// The task moved.
    pub task: String,
    /// The column it was dropped in.
    pub column: String,
    /// Its place there, counted without it.
    pub index: usize,
}

/// The card move the daemon hasn't answered (a newer drop replaces it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CardMoves {
    shown: Option<CardMove>,
    last: u64,
}

impl CardMoves {
    /// Shows `task` in `column` at `index` until answered; returns the move's number.
    pub fn start(&mut self, task: String, column: String, index: usize) -> u64 {
        self.last += 1;
        self.shown = Some(CardMove {
            query: self.last,
            task,
            column,
            index,
        });
        self.last
    }

    /// The daemon answered move `query`: ends its preview if it is the one shown.
    /// Returns whether it was.
    pub fn answered(&mut self, query: u64) -> bool {
        if self.shown.as_ref().is_some_and(|m| m.query == query) {
            self.shown = None;
            true
        } else {
            false
        }
    }

    /// The move shown, if any.
    pub fn shown(&self) -> Option<&CardMove> {
        self.shown.as_ref()
    }

    /// Shows the unanswered move on the daemon's `workspace`.
    pub fn apply(&self, workspace: &mut v1::Workspace) {
        let Some(m) = &self.shown else {
            return;
        };
        if let Some(board) = workspace.boards.iter_mut().find(|b| {
            b.columns
                .iter()
                .any(|c| c.tasks.iter().any(|t| t.id == m.task))
        }) {
            move_card(board, &m.task, &m.column, m.index);
        }
    }
}

/// Takes card `task` out of its column and puts it in `column` at `index` (counted
/// without it; past the end puts it last). Does nothing if either isn't on the board.
pub fn move_card(board: &mut v1::Board, task: &str, column: &str, index: usize) {
    if !board.columns.iter().any(|c| c.id == column) {
        return;
    }
    let Some(card) = board.columns.iter_mut().find_map(|c| {
        let at = c.tasks.iter().position(|t| t.id == task)?;
        Some(c.tasks.remove(at))
    }) else {
        return;
    };
    if let Some(target) = board.columns.iter_mut().find(|c| c.id == column) {
        let at = index.min(target.tasks.len());
        target.tasks.insert(at, card);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> v1::Board {
        let task = |id: &str| v1::Task {
            id: id.into(),
            ..Default::default()
        };
        v1::Board {
            columns: vec![
                v1::BoardColumn {
                    id: "todo".into(),
                    tasks: vec![task("a"), task("b"), task("c")],
                    ..Default::default()
                },
                v1::BoardColumn {
                    id: "done".into(),
                    tasks: vec![task("d")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn ids(b: &v1::Board, column: usize) -> Vec<&str> {
        b.columns[column]
            .tasks
            .iter()
            .map(|t| t.id.as_str())
            .collect()
    }

    #[test]
    fn a_card_moves_as_the_daemon_moves_it() {
        let mut b = board();
        move_card(&mut b, "a", "todo", 2);
        assert_eq!(ids(&b, 0), ["b", "c", "a"], "counted without it");
        move_card(&mut b, "c", "done", 0);
        assert_eq!(ids(&b, 1), ["c", "d"]);
        move_card(&mut b, "b", "done", usize::MAX);
        assert_eq!(ids(&b, 1), ["c", "d", "b"]);
        let before = b.clone();
        move_card(&mut b, "nope", "done", 0);
        move_card(&mut b, "a", "nope", 0);
        assert_eq!(b, before, "nothing missing is moved");
    }

    #[test]
    fn only_the_newest_move_shows_and_ends_with_its_answer() {
        let mut moves = CardMoves::default();
        let first = moves.start("a".into(), "done".into(), 0);
        let second = moves.start("b".into(), "done".into(), 0);
        assert!(!moves.answered(first));
        assert_eq!(moves.shown().map(|m| m.task.as_str()), Some("b"));
        assert!(moves.answered(second));
        assert!(moves.shown().is_none());
    }
}
