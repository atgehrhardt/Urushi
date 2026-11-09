//! Typed wrappers for task-board and agent-usage requests (protocol 1.5; subtasks since
//! 1.6).

use urushi_proto::v1::{self, request::Body, response::Outcome};

use crate::requests::unexpected;
use crate::{Client, ClientError};

impl Client {
    /// Creates a board with the default columns.
    pub async fn create_board(&self, name: &str) -> Result<v1::Board, ClientError> {
        let body = Body::CreateBoard(v1::CreateBoardRequest { name: name.into() });
        match self.request(body).await? {
            Outcome::Board(board) => Ok(board),
            _ => Err(unexpected("CreateBoard")),
        }
    }

    /// Renames a board.
    pub async fn rename_board(&self, board_id: &str, name: &str) -> Result<(), ClientError> {
        let body = Body::RenameBoard(v1::RenameBoardRequest {
            board_id: board_id.into(),
            name: name.into(),
        });
        self.ok(body, "RenameBoard").await
    }

    /// Deletes a board and its tasks.
    pub async fn delete_board(&self, board_id: &str) -> Result<(), ClientError> {
        let body = Body::DeleteBoard(v1::DeleteBoardRequest {
            board_id: board_id.into(),
        });
        self.ok(body, "DeleteBoard").await
    }

    /// Adds a task at the bottom of a column.
    pub async fn create_task(
        &self,
        board_id: &str,
        column_id: &str,
        title: &str,
        notes: &str,
    ) -> Result<v1::Task, ClientError> {
        let body = Body::CreateTask(v1::CreateTaskRequest {
            board_id: board_id.into(),
            column_id: column_id.into(),
            title: title.into(),
            notes: notes.into(),
            subtasks: Vec::new(),
        });
        match self.request(body).await? {
            Outcome::Task(task) => Ok(task),
            _ => Err(unexpected("CreateTask")),
        }
    }

    /// Adds a subtask at the end of a task's checklist (protocol 1.6).
    pub async fn create_subtask(
        &self,
        task_id: &str,
        title: &str,
    ) -> Result<v1::Subtask, ClientError> {
        let body = Body::CreateSubtask(v1::CreateSubtaskRequest {
            task_id: task_id.into(),
            title: title.into(),
        });
        match self.request(body).await? {
            Outcome::Subtask(subtask) => Ok(subtask),
            _ => Err(unexpected("CreateSubtask")),
        }
    }

    /// Renames and/or checks off a subtask (`None` keeps the value).
    pub async fn update_subtask(
        &self,
        subtask_id: &str,
        title: Option<&str>,
        done: Option<bool>,
    ) -> Result<(), ClientError> {
        let body = Body::UpdateSubtask(v1::UpdateSubtaskRequest {
            subtask_id: subtask_id.into(),
            title: title.map(Into::into),
            done,
        });
        self.ok(body, "UpdateSubtask").await
    }

    /// Deletes a subtask.
    pub async fn delete_subtask(&self, subtask_id: &str) -> Result<(), ClientError> {
        let body = Body::DeleteSubtask(v1::DeleteSubtaskRequest {
            subtask_id: subtask_id.into(),
        });
        self.ok(body, "DeleteSubtask").await
    }

    /// Changes a task's title and/or notes (`None` keeps them).
    pub async fn update_task(
        &self,
        task_id: &str,
        title: Option<&str>,
        notes: Option<&str>,
    ) -> Result<(), ClientError> {
        let body = Body::UpdateTask(v1::UpdateTaskRequest {
            task_id: task_id.into(),
            title: title.map(Into::into),
            notes: notes.map(Into::into),
        });
        self.ok(body, "UpdateTask").await
    }

    /// Moves a task to `index` in a column of its board.
    pub async fn move_task(
        &self,
        task_id: &str,
        column_id: &str,
        index: u32,
    ) -> Result<(), ClientError> {
        let body = Body::MoveTask(v1::MoveTaskRequest {
            task_id: task_id.into(),
            column_id: column_id.into(),
            index,
        });
        self.ok(body, "MoveTask").await
    }

    /// Deletes a task.
    pub async fn delete_task(&self, task_id: &str) -> Result<(), ClientError> {
        let body = Body::DeleteTask(v1::DeleteTaskRequest {
            task_id: task_id.into(),
        });
        self.ok(body, "DeleteTask").await
    }

    /// Which agent runs in a pane, and how much of its plan is used.
    pub async fn agent_usage(&self, pane_id: &str) -> Result<v1::AgentUsage, ClientError> {
        let body = Body::GetAgentUsage(v1::GetAgentUsageRequest {
            pane_id: pane_id.into(),
        });
        match self.request(body).await? {
            Outcome::AgentUsage(usage) => Ok(usage),
            _ => Err(unexpected("GetAgentUsage")),
        }
    }
}
