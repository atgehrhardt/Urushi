//! Subtasks: a task's checklist of smaller steps, and the links from tasks and subtasks
//! to the threads started for them.
//!
//! A subtask belongs to exactly one task and moves with it. Like tasks, subtasks only
//! organize work: a link names a thread but never creates, changes or deletes one. The
//! daemon clears links when their thread is deleted ([`Board::unlink_thread`]).

use serde::{Deserialize, Serialize};

use super::{Board, BoardError, Task, task_title};
use crate::{SubtaskId, TaskId, ThreadId};

/// Most subtasks one task can have.
pub const MAX_SUBTASKS: usize = 100;

/// One step of a task's checklist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subtask {
    /// Identity.
    pub id: SubtaskId,
    /// One-line summary.
    pub title: String,
    /// Checked off.
    #[serde(default)]
    pub done: bool,
    /// The thread started for this subtask, if any. It may have been deleted since.
    #[serde(default)]
    pub thread: Option<ThreadId>,
}

impl Subtask {
    /// An unchecked subtask with a validated title (same rules as task titles).
    pub fn new(id: SubtaskId, title: &str) -> Result<Self, BoardError> {
        Ok(Self {
            id,
            title: task_title(title)?,
            done: false,
            thread: None,
        })
    }
}

impl Task {
    /// `(checked, total)` subtasks, for progress like "2/5".
    pub fn subtasks_done(&self) -> (usize, usize) {
        let done = self.subtasks.iter().filter(|s| s.done).count();
        (done, self.subtasks.len())
    }
}

impl Board {
    /// The subtask with this id and the task it belongs to.
    pub fn subtask(&self, id: SubtaskId) -> Option<(&Task, &Subtask)> {
        self.columns
            .iter()
            .flat_map(|c| &c.tasks)
            .find_map(|t| t.subtasks.iter().find(|s| s.id == id).map(|s| (t, s)))
    }

    /// Adds `subtask` at the end of `task`'s checklist.
    pub fn add_subtask(&mut self, task: TaskId, subtask: Subtask) -> Result<(), BoardError> {
        let task = self.task_mut(task)?;
        if task.subtasks.len() >= MAX_SUBTASKS {
            return Err(BoardError::TooManySubtasks);
        }
        task.subtasks.push(subtask);
        Ok(())
    }

    /// Renames and/or checks a subtask; `None` keeps the current value. Nothing changes
    /// unless the title is valid.
    pub fn update_subtask(
        &mut self,
        id: SubtaskId,
        title: Option<&str>,
        done: Option<bool>,
    ) -> Result<(), BoardError> {
        let title = title.map(task_title).transpose()?;
        let subtask = self.subtask_mut(id)?;
        if let Some(title) = title {
            subtask.title = title;
        }
        if let Some(done) = done {
            subtask.done = done;
        }
        Ok(())
    }

    /// Removes a subtask and returns it.
    pub fn remove_subtask(&mut self, id: SubtaskId) -> Result<Subtask, BoardError> {
        for task in self.columns.iter_mut().flat_map(|c| &mut c.tasks) {
            if let Some(at) = task.subtasks.iter().position(|s| s.id == id) {
                return Ok(task.subtasks.remove(at));
            }
        }
        Err(BoardError::NoSuchSubtask(id))
    }

    /// Records that `thread` was started for `task`, or for its `subtask`.
    pub fn link_thread(
        &mut self,
        task: TaskId,
        subtask: Option<SubtaskId>,
        thread: ThreadId,
    ) -> Result<(), BoardError> {
        let parent = self.task_mut(task)?;
        match subtask {
            None => parent.thread = Some(thread),
            Some(id) => {
                parent
                    .subtasks
                    .iter_mut()
                    .find(|s| s.id == id)
                    .ok_or(BoardError::NoSuchSubtask(id))?
                    .thread = Some(thread);
            }
        }
        Ok(())
    }

    /// Forgets every link to `thread` (it was deleted). Returns whether there was one.
    pub fn unlink_thread(&mut self, thread: ThreadId) -> bool {
        let mut found = false;
        for task in self.columns.iter_mut().flat_map(|c| &mut c.tasks) {
            let links = std::iter::once(&mut task.thread)
                .chain(task.subtasks.iter_mut().map(|s| &mut s.thread));
            for link in links {
                if *link == Some(thread) {
                    *link = None;
                    found = true;
                }
            }
        }
        found
    }

    fn subtask_mut(&mut self, id: SubtaskId) -> Result<&mut Subtask, BoardError> {
        self.columns
            .iter_mut()
            .flat_map(|c| &mut c.tasks)
            .flat_map(|t| &mut t.subtasks)
            .find(|s| s.id == id)
            .ok_or(BoardError::NoSuchSubtask(id))
    }
}
