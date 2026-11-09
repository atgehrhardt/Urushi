//! `urushi board …` (task boards and their subtasks) and `urushi usage` (an agent's plan
//! usage).

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use urushi_client::{Client, WorkspaceMirror, v1};

use crate::commands::{out, resolve};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum BoardCmd {
    /// List boards, their columns and tasks.
    Ls,
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
    },
    /// Change a task's title or notes.
    Edit {
        /// Task id.
        task: String,
        /// New title.
        #[arg(long)]
        title: Option<String>,
        /// New notes.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Move a task to a column of its board.
    Mv {
        /// Task id.
        task: String,
        /// Column name or id.
        column: String,
        /// Position in the column (0 = top; default: bottom).
        #[arg(long)]
        index: Option<u32>,
    },
    /// Delete a task.
    RmTask {
        /// Task id.
        task: String,
    },
    /// Manage a task's subtasks (its checklist).
    #[command(subcommand)]
    Sub(SubCmd),
}

#[derive(Subcommand)]
pub(crate) enum SubCmd {
    /// Add a subtask at the end of a task's checklist.
    Add {
        /// Task id.
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
        } => {
            let board = resolve(client, &board).await?;
            let column = column_id(client, &board, &column).await?;
            let task = client.create_task(&board, &column, &title, &notes).await?;
            out(&format!("task {} {}", short(&task.id), task.title))
        }
        BoardCmd::Edit { task, title, notes } => Ok(client
            .update_task(
                &resolve(client, &task).await?,
                title.as_deref(),
                notes.as_deref(),
            )
            .await?),
        BoardCmd::Mv {
            task,
            column,
            index,
        } => {
            let task = resolve(client, &task).await?;
            let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
            let board = mirror.task(&task).context("no such task")?.0.id.clone();
            let column = column_id(client, &board, &column).await?;
            Ok(client
                .move_task(&task, &column, index.unwrap_or(u32::MAX))
                .await?)
        }
        BoardCmd::RmTask { task } => Ok(client.delete_task(&resolve(client, &task).await?).await?),
        BoardCmd::Sub(cmd) => sub(client, cmd).await,
    }
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
async fn column_id(client: &Client, board: &str, column: &str) -> Result<String> {
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

fn print_boards(mirror: &WorkspaceMirror) -> Result<()> {
    if mirror.boards().is_empty() {
        return out("no boards (create one with `urushi board new <name>`)");
    }
    for board in mirror.boards() {
        out(&format!("board {} {}", short(&board.id), board.name))?;
        for column in &board.columns {
            out(&format!(
                "  {} ({})  [{}]",
                column.name,
                column.tasks.len(),
                short(&column.id)
            ))?;
            for task in &column.tasks {
                let thread = link(mirror, &task.thread_id);
                out(&format!(
                    "    - {}  [{}]{thread}",
                    task.title,
                    short(&task.id)
                ))?;
                for line in task.notes.lines() {
                    out(&format!("        {line}"))?;
                }
                for s in &task.subtasks {
                    let mark = if s.done { "x" } else { " " };
                    let thread = link(mirror, &s.thread_id);
                    out(&format!(
                        "        [{mark}] {}  [{}]{thread}",
                        s.title,
                        short(&s.id)
                    ))?;
                }
            }
        }
    }
    Ok(())
}

/// " → thread <title>" for a task's or subtask's thread, if it still exists.
fn link(mirror: &WorkspaceMirror, thread: &str) -> String {
    mirror
        .thread(thread)
        .map(|t| format!("  → thread {} [{}]", t.title, short(&t.id)))
        .unwrap_or_default()
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
