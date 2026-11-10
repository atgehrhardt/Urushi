//! Managing a board's columns: adding, renaming, coloring, reordering and removing
//! them. A board always keeps at least one column, and removing a column never removes
//! its tasks: they move to the column next to it.

use super::labels::LabelColor;
use super::{Board, BoardError, Column, board_name};
use crate::ColumnId;

/// Most columns one board can have.
pub const MAX_COLUMNS: usize = 20;

impl Board {
    /// The column with this id.
    pub fn column(&self, id: ColumnId) -> Option<&Column> {
        self.columns.iter().find(|c| c.id == id)
    }

    /// Adds an empty column named `name` at `index` (clamped; past the end means last).
    pub fn add_column(&mut self, id: ColumnId, name: &str, index: usize) -> Result<(), BoardError> {
        self.local_only("changing columns")?;
        let name = column_name(name)?;
        if self.columns.len() >= MAX_COLUMNS {
            return Err(BoardError::TooManyColumns);
        }
        let column = Column {
            id,
            name,
            color: None,
            tasks: Vec::new(),
            statuses: Vec::new(),
        };
        self.columns.insert(index.min(self.columns.len()), column);
        Ok(())
    }

    /// Renames and/or recolors a column. `None` keeps the value; `Some(None)` clears the
    /// color. Nothing changes unless the name is valid.
    pub fn update_column(
        &mut self,
        id: ColumnId,
        name: Option<&str>,
        color: Option<Option<LabelColor>>,
    ) -> Result<(), BoardError> {
        if name.is_some() {
            self.local_only("renaming a column")?;
        }
        let name = name.map(column_name).transpose()?;
        let column = self.column_mut(id)?;
        if let Some(name) = name {
            column.name = name;
        }
        if let Some(color) = color {
            column.color = color;
        }
        Ok(())
    }

    /// Moves a column to `index` (counted without it; clamped to the end).
    pub fn move_column(&mut self, id: ColumnId, index: usize) -> Result<(), BoardError> {
        self.local_only("changing columns")?;
        let at = self.column_index(id)?;
        let column = self.columns.remove(at);
        self.columns.insert(index.min(self.columns.len()), column);
        Ok(())
    }

    /// The column a removed column's tasks would go to: the one on its left, or on its
    /// right for the first column. `None` if it is the only column.
    pub fn column_heir(&self, id: ColumnId) -> Result<Option<ColumnId>, BoardError> {
        let at = self.column_index(id)?;
        let heir = if at > 0 { at - 1 } else { at + 1 };
        Ok(self.columns.get(heir).map(|c| c.id))
    }

    /// Removes a column, moving its tasks (in order) to the bottom of its
    /// [`column_heir`](Self::column_heir). Returns where they went. The last column
    /// can't be removed.
    pub fn remove_column(&mut self, id: ColumnId) -> Result<ColumnId, BoardError> {
        self.local_only("changing columns")?;
        let heir = self.column_heir(id)?.ok_or(BoardError::LastColumn)?;
        let at = self.column_index(id)?;
        let removed = self.columns.remove(at);
        self.column_mut(heir)?.tasks.extend(removed.tasks);
        Ok(heir)
    }

    fn column_index(&self, id: ColumnId) -> Result<usize, BoardError> {
        self.columns
            .iter()
            .position(|c| c.id == id)
            .ok_or(BoardError::NoSuchColumn(id))
    }
}

/// A column name: same rules as board names.
fn column_name(name: &str) -> Result<String, BoardError> {
    board_name(name).map_err(|e| match e {
        BoardError::EmptyName => BoardError::EmptyColumnName,
        BoardError::TooLong { max, .. } => BoardError::TooLong {
            what: "a column name",
            max,
        },
        e => e,
    })
}
