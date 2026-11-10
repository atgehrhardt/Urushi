//! `urushi board …` (task boards, their columns, tasks and subtasks) and `urushi usage` (an agent's plan
//! usage).

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use urushi_client::{Client, WorkspaceMirror, v1};

use crate::commands::{out, resolve};
use crate::render::short;

mod attachments;
mod columns;
mod print;
mod show;

use attachments::AttachmentCmd;
use columns::{ColCmd, color_arg};
use print::print_boards;

#[derive(Subcommand)]
pub(crate) enum BoardCmd {
    /// List boards, their columns and tasks.
    Ls,
    /// Print a task in full: where it is, its notes and subtasks, and for a Jira issue
    /// its fields, comments, links and files (what agents read to review a task).
    Show {
        /// A task or subtask id (or its start), or a Jira issue key like UR-12.
        task: String,
    },
    /// Create a board (with To do, In progress and Done columns).
    New {
        /// Name.
        name: String,
    },
    /// Rename a board.
    Rename {
        /// Board id.
        board: String,
        /// New name.
        name: String,
    },
    /// Delete a board and all its tasks.
    Rm {
        /// Board id.
        board: String,
    },
    /// Add a task at the bottom of a column.
    Add {
        /// Board id.
        board: String,
        /// Column name (e.g. "To do") or id.
        column: String,
        /// Title.
        title: String,
        /// Notes.
        #[arg(long, default_value = "")]
        notes: String,
        /// Color: red, orange, yellow, green, teal, blue, purple, pink or gray.
        #[arg(long)]
        color: Option<String>,
        /// A tag (repeat for more).
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Kind: task, story, bug or epic (on a Jira board, the issue type).
        #[arg(long)]
        kind: Option<String>,
    },
    /// Change a task's title, notes, color, tags or kind.
    Edit {
        /// Task id.
        task: String,
        /// New title.
        #[arg(long)]
        title: Option<String>,
        /// New notes.
        #[arg(long)]
        notes: Option<String>,
        /// New color, or "none" to clear it.
        #[arg(long)]
        color: Option<String>,
        /// Add a tag (repeat for more).
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Remove a tag (repeat for more).
        #[arg(long = "untag")]
        untags: Vec<String>,
        /// New kind (task, story, bug, epic, subtask), or "none" to clear it.
        #[arg(long)]
        kind: Option<String>,
    },
    /// Move a task (or a subtask, which then becomes a card) to a column of its board.
    Mv {
        /// Task id.
        task: String,
        /// Column name or id.
        column: String,
        /// Position in the column (0 = top; default: bottom).
        #[arg(long)]
        index: Option<u32>,
    },
    /// Make a task (a card or a subtask) a subtask of another task on its board.
    Nest {
        /// Task id.
        task: String,
        /// The new parent's task id.
        parent: String,
        /// Position among the parent's subtasks (0 = first; default: last).
        #[arg(long)]
        index: Option<u32>,
    },
    /// Delete a task (or a subtask) with its subtasks.
    RmTask {
        /// Task id.
        task: String,
    },
    /// Manage a task's subtasks.
    #[command(subcommand)]
    Sub(SubCmd),
    /// Manage a board's columns.
    #[command(subcommand)]
    Col(ColCmd),
    /// Attach a file to a task or subtask (at most 6 MiB).
    Attach {
        /// Task or subtask id.
        task: String,
        /// The file.
        file: String,
    },
    /// Fetch or delete an attachment.
    #[command(subcommand)]
    Attachment(AttachmentCmd),
    /// Sync a Jira board now, completely.
    Sync {
        /// Board id.
        board: String,
    },
}

/// A `--kind` value as a wire kind; "none" is unspecified.
fn kind_arg(kind: &str) -> Result<i32> {
    if kind.trim().eq_ignore_ascii_case("none") {
        return Ok(v1::TaskKind::Unspecified.into());
    }
    let kind = urushi_core::TaskKind::parse(kind)
        .with_context(|| format!("{kind:?} isn't a kind: task, story, bug, epic or subtask"))?;
    Ok(urushi_proto::convert::kind_to_proto(Some(kind)))
}

#[derive(Subcommand)]
pub(crate) enum SubCmd {
    /// Add a subtask at the end of a task's (or a subtask's) subtasks.
    Add {
        /// Task or subtask id.
        task: String,
        /// Title.
        title: String,
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
    /// Delete a subtask.
    Rm {
        /// Subtask id.
        subtask: String,
    },
}

pub(crate) async fn run(client: &Client, cmd: BoardCmd) -> Result<()> {
    match cmd {
        BoardCmd::Ls => print_boards(&WorkspaceMirror::new(client.list_workspace(false).await?)),
        BoardCmd::Show { task } => show::show(client, &task).await,
        BoardCmd::New { name } => {
            let board = client.create_board(&name).await?;
            out(&format!("board {} {}", short(&board.id), board.name))
        }
        BoardCmd::Rename { board, name } => Ok(client
            .rename_board(&resolve(client, &board).await?, &name)
            .await?),
        BoardCmd::Rm { board } => Ok(client.delete_board(&resolve(client, &board).await?).await?),
        BoardCmd::Add {
            board,
            column,
            title,
            notes,
            color,
            tags,
            kind,
        } => {
            let board = resolve(client, &board).await?;
            let column = column_id(client, &board, &column).await?;
            let task = client
                .create_task_with(v1::CreateTaskRequest {
                    board_id: board,
                    column_id: column,
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
                    ..Default::default()
                })
                .await?;
            let key = task
                .issue
                .as_ref()
                .map(|i| format!(" {}", i.key))
                .unwrap_or_default();
            out(&format!("task {}{key} {}", short(&task.id), task.title))
        }
        cmd @ BoardCmd::Edit { .. } => edit(client, cmd).await,
        BoardCmd::Mv {
            task,
            column,
            index,
        } => {
            let task = resolve(client, &task).await?;
            let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
            let board = mirror
                .find_task(&task)
                .context("no such task")?
                .board
                .id
                .clone();
            let column = column_id(client, &board, &column).await?;
            Ok(client
                .move_task(&task, &column, index.unwrap_or(u32::MAX))
                .await?)
        }
        BoardCmd::Nest {
            task,
            parent,
            index,
        } => Ok(client
            .nest_task(
                &resolve(client, &task).await?,
                &resolve(client, &parent).await?,
                index.unwrap_or(u32::MAX),
            )
            .await?),
        BoardCmd::RmTask { task } => Ok(client.delete_task(&resolve(client, &task).await?).await?),
        BoardCmd::Sub(cmd) => sub(client, cmd).await,
        BoardCmd::Col(cmd) => columns::run(client, cmd).await,
        BoardCmd::Attach { task, file } => attachments::attach(client, &task, &file).await,
        BoardCmd::Attachment(cmd) => attachments::run(client, cmd).await,
        BoardCmd::Sync { board } => {
            client.sync_board(&resolve(client, &board).await?).await?;
            out("synced")
        }
    }
}

/// `urushi board edit`: only the given fields change; tags are added and removed
/// against the task's current ones.
async fn edit(client: &Client, cmd: BoardCmd) -> Result<()> {
    let BoardCmd::Edit {
        task,
        title,
        notes,
        color,
        tags,
        untags,
        kind,
    } = cmd
    else {
        return Ok(());
    };
    let task = resolve(client, &task).await?;
    let tags = if tags.is_empty() && untags.is_empty() {
        None
    } else {
        let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
        let mut now = mirror
            .find_task(&task)
            .context("no such task")?
            .task()
            .tags()
            .to_vec();
        now.retain(|t| !untags.iter().any(|u| u.trim().eq_ignore_ascii_case(t)));
        now.extend(tags);
        Some(v1::TagList { tags: now })
    };
    Ok(client
        .update_task_with(v1::UpdateTaskRequest {
            task_id: task,
            title,
            notes,
            color: color.as_deref().map(color_arg).transpose()?.map(Into::into),
            tags,
            kind: kind.as_deref().map(kind_arg).transpose()?,
        })
        .await?)
}

async fn sub(client: &Client, cmd: SubCmd) -> Result<()> {
    match cmd {
        SubCmd::Add { task, title } => {
            let s = client
                .create_subtask(&resolve(client, &task).await?, &title)
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
        SubCmd::Rm { subtask } => Ok(client
            .delete_subtask(&resolve(client, &subtask).await?)
            .await?),
    }
}

/// A column of `board`, by name (any case) or id prefix.
pub(super) async fn column_id(client: &Client, board: &str, column: &str) -> Result<String> {
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    let board = mirror.board(board).context("no such board")?;
    let matches: Vec<&v1::BoardColumn> = board
        .columns
        .iter()
        .filter(|c| c.name.eq_ignore_ascii_case(column) || c.id.starts_with(column))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => bail!("no column {column:?} on board {}", board.name),
        _ => bail!("{column:?} matches more than one column"),
    }
}

/// Prints which agent runs in `pane` and its plan usage.
pub(crate) async fn usage(client: &Client, pane: &str) -> Result<()> {
    let usage = client.agent_usage(pane).await?;
    let agent = usage.agent();
    if agent == v1::Agent::Unspecified {
        return out("no agent running in this pane");
    }
    let name = agent
        .as_str_name()
        .trim_start_matches("AGENT_")
        .to_lowercase();
    if usage.plan.is_empty() {
        out(&name)?;
    } else {
        out(&format!("{name} ({})", usage.plan))?;
    }
    if !usage.unavailable.is_empty() {
        return out(&format!("  usage unavailable: {}", usage.unavailable));
    }
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(i64::MAX);
    for w in &usage.windows {
        let resets = if w.resets_at_ms > now {
            let minutes = (w.resets_at_ms - now) / 60_000;
            format!(
                ", resets in {}d {}h {}m",
                minutes / 1440,
                minutes % 1440 / 60,
                minutes % 60
            )
        } else {
            String::new()
        };
        out(&format!(
            "  {:<14} {:>5.1}% used{resets}",
            w.label, w.used_percent
        ))?;
    }
    Ok(())
}
