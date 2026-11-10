//! `urushi board sub …`: a task's subtasks (created with their details since protocol
//! 1.14, checked off, renamed and deleted).

use anyhow::Result;
use clap::Subcommand;
use urushi_client::{Client, v1};

use super::columns::color_arg;
use super::kind_arg;
use crate::commands::{out, resolve};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum SubCmd {
    /// Add a subtask at the end of a task's (or a subtask's) subtasks.
    Add {
        /// Task or subtask id.
        task: String,
        /// Title.
        title: String,
        /// Notes (protocol 1.14).
        #[arg(long, default_value = "")]
        notes: String,
        /// Color: red, orange, yellow, green, teal, blue, purple, pink or gray.
        #[arg(long)]
        color: Option<String>,
        /// A tag (repeat for more).
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Kind: task, story, bug or epic (not on a Jira board).
        #[arg(long)]
        kind: Option<String>,
    },
    /// Check a subtask off.
    Done {
        /// Subtask id.
        subtask: String,
    },
    /// Uncheck a subtask.
    Undo {
        /// Subtask id.
        subtask: String,
    },
    /// Rename a subtask.
    Rename {
        /// Subtask id.
        subtask: String,
        /// New title.
        title: String,
    },
    /// Delete a subtask. A Jira issue is deleted in Jira, for everyone, after a
    /// question.
    Rm {
        /// Subtask id.
        subtask: String,
        /// Don't ask before deleting a Jira issue.
        #[arg(long)]
        yes: bool,
    },
}

pub(super) async fn sub(client: &Client, cmd: SubCmd) -> Result<()> {
    match cmd {
        SubCmd::Add {
            task,
            title,
            notes,
            color,
            tags,
            kind,
        } => {
            let s = client
                .create_subtask_with(v1::CreateSubtaskRequest {
                    task_id: resolve(client, &task).await?,
                    title,
                    notes,
                    color: color
                        .as_deref()
                        .map(color_arg)
                        .transpose()?
                        .unwrap_or_default()
                        .into(),
                    tags,
                    kind: kind
                        .as_deref()
                        .map(kind_arg)
                        .transpose()?
                        .unwrap_or_default(),
                    jira_fields: Vec::new(),
                })
                .await?;
            out(&format!("subtask {} {}", short(&s.id), s.title))
        }
        SubCmd::Done { subtask } => Ok(client
            .update_subtask(&resolve(client, &subtask).await?, None, Some(true))
            .await?),
        SubCmd::Undo { subtask } => Ok(client
            .update_subtask(&resolve(client, &subtask).await?, None, Some(false))
            .await?),
        SubCmd::Rename { subtask, title } => Ok(client
            .update_subtask(&resolve(client, &subtask).await?, Some(&title), None)
            .await?),
        SubCmd::Rm { subtask, yes } => {
            let id = resolve(client, &subtask).await?;
            super::confirm_jira_delete(client, &id, yes).await?;
            Ok(client.delete_subtask(&id).await?)
        }
    }
}
