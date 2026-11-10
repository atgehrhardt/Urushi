//! Subtasks: a task's smaller steps, each a full [`Task`] kept inside its parent
//! (ADR 0015), and the links from tasks to the threads started for them.
//!
//! The board is a forest: columns hold top-level tasks, and each task holds its
//! subtasks in order. A task is in exactly one place, a column or one parent, and keeps
//! its id when it moves between them: [`Board::move_task`] puts any task into a column,
//! [`Board::nest_task`] under a parent. Nesting never makes a cycle, stays on one board,
//! and is bounded by [`MAX_DEPTH`] and [`MAX_SUBTASKS`]. A nested task's progress is its
//! `done` check mark; a top-level task's is its column.
//!
//! Like tasks, links only organize work: a link names a thread but never creates,
//! changes or deletes one. The daemon clears links when their thread is deleted
//! ([`Board::unlink_thread`]).

use super::{Board, BoardError, Task};
use crate::{ColumnId, TaskId, ThreadId};

/// Most subtasks one task can have.
pub const MAX_SUBTASKS: usize = 100;
/// Most levels of subtasks under a card on the board.
pub const MAX_DEPTH: usize = 5;

impl Task {
    /// `(checked, total)` direct subtasks, for progress like "2/5".
    pub fn subtasks_done(&self) -> (usize, usize) {
        let done = self.subtasks.iter().filter(|s| s.done).count();
        (done, self.subtasks.len())
    }

    /// This task and every task under it, parents first.
    pub fn subtree(&self) -> Vec<&Task> {
        let mut out = vec![self];
        for s in &self.subtasks {
            out.extend(s.subtree());
        }
        out
    }

    /// This task, or the task under it with this id.
    pub fn find(&self, id: TaskId) -> Option<&Task> {
        if self.id == id {
            return Some(self);
        }
        self.subtasks.iter().find_map(|s| s.find(id))
    }

    fn find_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        if self.id == id {
            return Some(self);
        }
        self.subtasks.iter_mut().find_map(|s| s.find_mut(id))
    }

    /// Levels of subtasks under this task (0 without any).
    pub fn height(&self) -> usize {
        self.subtasks
            .iter()
            .map(|s| s.height() + 1)
            .max()
            .unwrap_or(0)
    }

    /// Removes the task with this id from under this one.
    fn take(&mut self, id: TaskId) -> Option<Task> {
        if let Some(at) = self.subtasks.iter().position(|s| s.id == id) {
            return Some(self.subtasks.remove(at));
        }
        self.subtasks.iter_mut().find_map(|s| s.take(id))
    }

    /// Pushes the ancestors of `id` (from this task down), then the task itself, onto
    /// `path`. Returns whether it is under this task.
    fn path<'a>(&'a self, id: TaskId, path: &mut Vec<&'a Task>) -> bool {
        path.push(self);
        if self.id == id || self.subtasks.iter().any(|s| s.path(id, path)) {
            return true;
        }
        path.pop();
        false
    }
}

impl Board {
    /// Every task on the board, at any depth, parents first.
    pub fn all_tasks(&self) -> impl Iterator<Item = &Task> {
        self.columns
            .iter()
            .flat_map(|c| &c.tasks)
            .flat_map(Task::subtree)
    }

    /// The task with this id, at any depth.
    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.columns
            .iter()
            .flat_map(|c| &c.tasks)
            .find_map(|t| t.find(id))
    }

    /// The task with this id and its ancestors: the card on the board first, the task
    /// itself last.
    pub fn path_to(&self, id: TaskId) -> Option<Vec<&Task>> {
        let mut path = Vec::new();
        self.columns
            .iter()
            .flat_map(|c| &c.tasks)
            .any(|t| t.path(id, &mut path))
            .then_some(path)
    }

    /// The column of the card that holds this task (its own column, if it is a card).
    pub fn column_of(&self, id: TaskId) -> Option<ColumnId> {
        self.columns
            .iter()
            .find(|c| c.tasks.iter().any(|t| t.find(id).is_some()))
            .map(|c| c.id)
    }

    /// Adds `task` at the end of `parent`'s subtasks.
    pub fn add_subtask(&mut self, parent: TaskId, task: Task) -> Result<(), BoardError> {
        self.local_only("adding a subtask this way")?;
        self.check_room(parent, &task)?;
        self.task_mut(parent)?.subtasks.push(task);
        Ok(())
    }

    /// Moves a task (a card, or a subtask of any depth) under `parent`, at `index` of
    /// its subtasks (counted without the task; clamped to the end). A card from the
    /// board's last column arrives checked off.
    pub fn nest_task(
        &mut self,
        id: TaskId,
        parent: TaskId,
        index: usize,
    ) -> Result<(), BoardError> {
        self.local_only("nesting tasks")?;
        let task = self.task(id).ok_or(BoardError::NoSuchTask(id))?;
        if task.find(parent).is_some() {
            return Err(BoardError::Cycle);
        }
        let moving_within = self
            .task(parent)
            .ok_or(BoardError::NoSuchTask(parent))?
            .subtasks
            .iter()
            .any(|s| s.id == id);
        // Reordering under the same parent needs no room; anything else is checked
        // before the task leaves its place, so a refusal changes nothing.
        if !moving_within {
            self.check_room(parent, task)?;
        }
        let card_in = self
            .columns
            .iter()
            .position(|c| c.tasks.iter().any(|t| t.id == id));
        let mut task = self.detach(id)?;
        if let Some(column) = card_in {
            task.done = column + 1 == self.columns.len();
        }
        let siblings = &mut self.task_mut(parent)?.subtasks;
        siblings.insert(index.min(siblings.len()), task);
        Ok(())
    }

    /// Records that `thread` was started for this task (at any depth).
    pub fn link_thread(&mut self, id: TaskId, thread: ThreadId) -> Result<(), BoardError> {
        self.task_mut(id)?.thread = Some(thread);
        Ok(())
    }

    /// Forgets every link to `thread` (it was deleted). Returns whether there was one.
    pub fn unlink_thread(&mut self, thread: ThreadId) -> bool {
        fn unlink(task: &mut Task, thread: ThreadId) -> bool {
            let mut found = false;
            if task.thread == Some(thread) {
                task.thread = None;
                found = true;
            }
            for s in &mut task.subtasks {
                found |= unlink(s, thread);
            }
            found
        }
        let mut found = false;
        for task in self.columns.iter_mut().flat_map(|c| &mut c.tasks) {
            found |= unlink(task, thread);
        }
        found
    }

    /// Whether `task` (with its own subtasks) fits under `parent`: few enough siblings,
    /// and not too deep.
    fn check_room(&self, parent: TaskId, task: &Task) -> Result<(), BoardError> {
        let path = self.path_to(parent).ok_or(BoardError::NoSuchTask(parent))?;
        let Some(parent) = path.last() else {
            return Err(BoardError::NoSuchTask(parent));
        };
        if parent.subtasks.len() >= MAX_SUBTASKS {
            return Err(BoardError::TooManySubtasks);
        }
        // `path.len()` is the parent's level counted from 1; the task goes one below.
        if path.len() + task.height() > MAX_DEPTH {
            return Err(BoardError::TooDeep);
        }
        Ok(())
    }

    /// The task with this id, at any depth, to change.
    pub(crate) fn task_mut(&mut self, id: TaskId) -> Result<&mut Task, BoardError> {
        self.columns
            .iter_mut()
            .flat_map(|c| &mut c.tasks)
            .find_map(|t| t.find_mut(id))
            .ok_or(BoardError::NoSuchTask(id))
    }

    /// Removes a task (a card, or a subtask of any depth) with its subtasks, and
    /// returns it. A Jira board refuses: its issues are deleted in Jira first, then
    /// forgotten with [`Board::forget_issue`].
    pub fn remove_task(&mut self, id: TaskId) -> Result<Task, BoardError> {
        self.local_only("deleting a task")?;
        self.detach(id)
    }

    /// Takes a task out of its place, to put it elsewhere or drop it.
    pub(crate) fn detach(&mut self, id: TaskId) -> Result<Task, BoardError> {
        for column in &mut self.columns {
            if let Some(at) = column.tasks.iter().position(|t| t.id == id) {
                return Ok(column.tasks.remove(at));
            }
        }
        self.columns
            .iter_mut()
            .flat_map(|c| &mut c.tasks)
            .find_map(|t| t.take(id))
            .ok_or(BoardError::NoSuchTask(id))
    }
}

#[cfg(test)]
mod tests;
