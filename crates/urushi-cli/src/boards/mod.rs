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
mod sub;

use attachments::AttachmentCmd;
use columns::{ColCmd, color_arg};
use print::print_boards;
use sub::{SubCmd, sub};

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
    /// Move a board's tab to a position among the others (0 is first; past the end is
    /// last).
    Order {
        /// Board id.
        board: String,
        /// New position.
        index: u32,
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
    /// Delete a task (or a subtask) with its subtasks. A Jira issue is deleted in Jira,
    /// for everyone, after a question.
    RmTask {
        /// Task id.
        task: String,
        /// Don't ask before deleting a Jira issue.
        #[arg(long)]
        yes: bool,
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
pub(super) fn kind_arg(kind: &str) -> Result<i32> {
    if kind.trim().eq_ignore_ascii_case("none") {
        return Ok(v1::TaskKind::Unspecified.into());
    }
    let kind = urushi_core::TaskKind::parse(kind)
        .with_context(|| format!("{kind:?} isn't a kind: task, story, bug, epic or subtask"))?;
    Ok(urushi_proto::convert::kind_to_proto(Some(kind)))
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
        BoardCmd::Order { board, index } => Ok(client
            .move_board(&resolve(client, &board).await?, index)
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
        BoardCmd::RmTask { task, yes } => {
            let id = resolve(client, &task).await?;
            confirm_jira_delete(client, &id, yes).await?;
            Ok(client.delete_task(&id).await?)
        }
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

/// Deleting a Jira issue deletes it in Jira, for everyone and for good, so it asks first
/// (ADR 0033); `--yes` skips the question. Run without a terminal to answer, it needs
/// `--yes`. Tasks of Urushi's own are deleted without asking.
pub(super) async fn confirm_jira_delete(client: &Client, id: &str, yes: bool) -> Result<()> {
    use std::io::IsTerminal;
    if yes {
        return Ok(());
    }
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    let Some(found) = mirror.find_task(id) else {
        return Ok(());
    };
    let task = found.task();
    let Some(issue) = task.issue() else {
        return Ok(());
    };
    let with = match task.subtree().len().saturating_sub(1) {
        0 => String::new(),
        1 => " and its subtask".to_owned(),
        n => format!(" and its {n} subtasks"),
    };
    let key = &issue.key;
    if !std::io::stdin().is_terminal() {
        bail!(
            "{key} is a Jira issue: deleting it deletes it in Jira for everyone. Pass --yes to delete it"
        );
    }
    let question = format!(
        "Delete {key} “{}”{with} from Jira? This can't be undone. [y/N] ",
        task.title()
    );
    if !crate::commands::confirm(&question)? {
        bail!("cancelled");
    }
    Ok(())
}
