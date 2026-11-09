//! Messages for acting on a board's cards from outside the task editor: the card
//! context menu (opened with a secondary click, i.e. a right or two-finger click).

/// Something done to a card on the board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardMsg {
    /// Open a task in the editor.
    OpenTask(String),
    /// Delete a task right away (from its menu, without opening it).
    DeleteTask(String),
    /// Move a task to the bottom of another column.
    MoveTask {
        /// Task id.
        task: String,
        /// Column id.
        column: String,
    },
}
