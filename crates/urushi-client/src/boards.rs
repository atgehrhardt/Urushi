//! Typed wrappers for task-board and agent-usage requests (protocol 1.5; subtasks since
//! 1.6; colors, tags and columns since 1.8).

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

    /// Moves a board to `index` among the others (counted without it; clamped to the
    /// end). Needs a 1.17 daemon.
    pub async fn move_board(&self, board_id: &str, index: u32) -> Result<(), ClientError> {
        let body = Body::MoveBoard(v1::MoveBoardRequest {
            board_id: board_id.into(),
            index,
        });
        self.ok(body, "MoveBoard").await
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
        self.create_task_with(v1::CreateTaskRequest {
            board_id: board_id.into(),
            column_id: column_id.into(),
            title: title.into(),
            notes: notes.into(),
            ..Default::default()
        })
        .await
    }

    /// Adds a task with everything a `CreateTask` can carry (color and tags since 1.8).
    pub async fn create_task_with(
        &self,
        request: v1::CreateTaskRequest,
    ) -> Result<v1::Task, ClientError> {
        match self.request(Body::CreateTask(request)).await? {
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
        self.create_subtask_with(v1::CreateSubtaskRequest {
            task_id: task_id.into(),
            title: title.into(),
            ..Default::default()
        })
        .await
    }

    /// Adds a subtask with everything a `CreateSubtask` can carry (notes, color, tags,
    /// kind and Jira fields since 1.14).
    pub async fn create_subtask_with(
        &self,
        request: v1::CreateSubtaskRequest,
    ) -> Result<v1::Subtask, ClientError> {
        match self.request(Body::CreateSubtask(request)).await? {
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

    /// Moves a task (a card or a subtask) under another task of its board, at `index`
    /// of its subtasks (protocol 1.8).
    pub async fn nest_task(
        &self,
        task_id: &str,
        parent_id: &str,
        index: u32,
    ) -> Result<(), ClientError> {
        let body = Body::NestTask(v1::NestTaskRequest {
            task_id: task_id.into(),
            parent_id: parent_id.into(),
            index,
        });
        self.ok(body, "NestTask").await
    }

    /// Attaches a file to a task or subtask (protocol 1.8).
    pub async fn add_attachment(
        &self,
        task_id: &str,
        name: &str,
        data: Vec<u8>,
    ) -> Result<v1::Attachment, ClientError> {
        let body = Body::AddAttachment(v1::AddAttachmentRequest {
            task_id: task_id.into(),
            name: name.into(),
            data,
        });
        match self.request(body).await? {
            Outcome::Attachment(a) => Ok(a),
            _ => Err(unexpected("AddAttachment")),
        }
    }

    /// An attachment's file, or with `thumbnail`, an image's thumbnail as raw RGBA.
    pub async fn get_attachment(
        &self,
        attachment_id: &str,
        thumbnail: bool,
    ) -> Result<v1::AttachmentData, ClientError> {
        let body = Body::GetAttachment(v1::GetAttachmentRequest {
            attachment_id: attachment_id.into(),
            thumbnail,
        });
        match self.request(body).await? {
            Outcome::AttachmentData(d) => Ok(d),
            _ => Err(unexpected("GetAttachment")),
        }
    }

    /// Deletes an attachment.
    pub async fn delete_attachment(&self, attachment_id: &str) -> Result<(), ClientError> {
        let body = Body::DeleteAttachment(v1::DeleteAttachmentRequest {
            attachment_id: attachment_id.into(),
        });
        self.ok(body, "DeleteAttachment").await
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
        self.update_task_with(v1::UpdateTaskRequest {
            task_id: task_id.into(),
            title: title.map(Into::into),
            notes: notes.map(Into::into),
            ..Default::default()
        })
        .await
    }

    /// Changes any of a task's fields (color and tags since 1.8); unset ones are kept.
    pub async fn update_task_with(
        &self,
        request: v1::UpdateTaskRequest,
    ) -> Result<(), ClientError> {
        self.ok(Body::UpdateTask(request), "UpdateTask").await
    }

    /// Adds an empty column at `index` of a board (protocol 1.8).
    pub async fn create_column(
        &self,
        board_id: &str,
        name: &str,
        index: u32,
    ) -> Result<v1::BoardColumn, ClientError> {
        let body = Body::CreateColumn(v1::CreateColumnRequest {
            board_id: board_id.into(),
            name: name.into(),
            index,
        });
        match self.request(body).await? {
            Outcome::Column(column) => Ok(column),
            _ => Err(unexpected("CreateColumn")),
        }
    }

    /// Renames and/or recolors a column (`None` keeps the value; `Some(Unspecified)`
    /// clears the color).
    pub async fn update_column(
        &self,
        column_id: &str,
        name: Option<&str>,
        color: Option<v1::LabelColor>,
    ) -> Result<(), ClientError> {
        let body = Body::UpdateColumn(v1::UpdateColumnRequest {
            column_id: column_id.into(),
            name: name.map(Into::into),
            color: color.map(Into::into),
        });
        self.ok(body, "UpdateColumn").await
    }

    /// Moves a column to `index` among the others.
    pub async fn move_column(&self, column_id: &str, index: u32) -> Result<(), ClientError> {
        let body = Body::MoveColumn(v1::MoveColumnRequest {
            column_id: column_id.into(),
            index,
        });
        self.ok(body, "MoveColumn").await
    }

    /// Removes a column; its tasks move to the column next to it.
    pub async fn delete_column(&self, column_id: &str) -> Result<(), ClientError> {
        let body = Body::DeleteColumn(v1::DeleteColumnRequest {
            column_id: column_id.into(),
        });
        self.ok(body, "DeleteColumn").await
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
