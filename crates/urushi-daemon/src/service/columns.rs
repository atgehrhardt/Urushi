//! A board's columns: adding, renaming, recoloring, reordering and removing them. The
//! rules (names, at least one column, where a removed column's tasks go) live in
//! `urushi_core::board`; this module finds the board and persists the change.

use urushi_core::{Board, BoardId, Column, ColumnId, LabelColor};

use super::{Service, ServiceError, ServiceResult, State, new_uuid};

impl State {
    /// The board holding `column`.
    fn board_of_column(&mut self, column: ColumnId) -> ServiceResult<&mut Board> {
        self.boards
            .iter_mut()
            .find(|b| b.column(column).is_some())
            .ok_or_else(|| ServiceError::NotFound(format!("column {column}")))
    }
}

impl Service {
    /// Adds an empty column at `index` of a board.
    pub fn create_column(&self, board: BoardId, name: &str, index: usize) -> ServiceResult<Column> {
        let id = ColumnId::from_uuid(new_uuid());
        self.mutate(|state| {
            let board = state.board_mut(board)?;
            board.add_column(id, name, index)?;
            board
                .column(id)
                .cloned()
                .ok_or_else(|| ServiceError::Internal("column just added is missing".into()))
        })
    }

    /// Renames and/or recolors a column (`None` keeps the value; `Some(None)` clears the
    /// color).
    pub fn update_column(
        &self,
        column: ColumnId,
        name: Option<&str>,
        color: Option<Option<LabelColor>>,
    ) -> ServiceResult<()> {
        self.mutate(|state| {
            Ok(state
                .board_of_column(column)?
                .update_column(column, name, color)?)
        })
    }

    /// Moves a column to `index` among the others.
    pub fn move_column(&self, column: ColumnId, index: usize) -> ServiceResult<()> {
        self.mutate(|state| Ok(state.board_of_column(column)?.move_column(column, index)?))
    }

    /// Removes a column, moving its tasks next door. Returns the column they went to.
    pub fn delete_column(&self, column: ColumnId) -> ServiceResult<ColumnId> {
        self.mutate(|state| Ok(state.board_of_column(column)?.remove_column(column)?))
    }
}
