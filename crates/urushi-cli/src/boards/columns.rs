//! `urushi board col …`: adding, renaming, recoloring, moving and removing a board's
//! columns (protocol 1.8).

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use urushi_client::{Client, WorkspaceMirror, v1};

use super::column_id;
use crate::commands::{out, resolve};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum ColCmd {
    /// Add a column.
    Add {
        /// Board id.
        board: String,
        /// Name.
        name: String,
        /// Position (0 = first; default: last).
        #[arg(long)]
        index: Option<u32>,
    },
    /// Rename a column.
    Rename {
        /// Board id.
        board: String,
        /// Column name or id.
        column: String,
        /// New name.
        name: String,
    },
    /// Color a column (red, orange, yellow, green, teal, blue, purple, pink, gray or none).
    Color {
        /// Board id.
        board: String,
        /// Column name or id.
        column: String,
        /// Color, or "none".
        color: String,
    },
    /// Move a column.
    Mv {
        /// Board id.
        board: String,
        /// Column name or id.
        column: String,
        /// New position (0 = first).
        index: u32,
    },
    /// Remove a column. Its tasks move to the column on its left (or right, if first).
    Rm {
        /// Board id.
        board: String,
        /// Column name or id.
        column: String,
    },
}

pub(super) async fn run(client: &Client, cmd: ColCmd) -> Result<()> {
    match cmd {
        ColCmd::Add { board, name, index } => {
            let board = resolve(client, &board).await?;
            let c = client
                .create_column(&board, &name, index.unwrap_or(u32::MAX))
                .await?;
            out(&format!("column {} {}", short(&c.id), c.name))
        }
        ColCmd::Rename {
            board,
            column,
            name,
        } => {
            let column = find(client, &board, &column).await?;
            Ok(client.update_column(&column, Some(&name), None).await?)
        }
        ColCmd::Color {
            board,
            column,
            color,
        } => {
            let column = find(client, &board, &column).await?;
            Ok(client
                .update_column(&column, None, Some(color_arg(&color)?))
                .await?)
        }
        ColCmd::Mv {
            board,
            column,
            index,
        } => {
            let column = find(client, &board, &column).await?;
            Ok(client.move_column(&column, index).await?)
        }
        ColCmd::Rm { board, column } => {
            let column = find(client, &board, &column).await?;
            let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
            let (_, c) = mirror.column(&column).context("no such column")?;
            let tasks = c.tasks.len();
            client.delete_column(&column).await?;
            if tasks > 0 {
                out(&format!("moved {tasks} task(s) to the next column"))?;
            }
            Ok(())
        }
    }
}

async fn find(client: &Client, board: &str, column: &str) -> Result<String> {
    let board = resolve(client, board).await?;
    column_id(client, &board, column).await
}

/// A color named on the command line; "none" is no color.
pub(super) fn color_arg(name: &str) -> Result<v1::LabelColor> {
    if name.trim().eq_ignore_ascii_case("none") {
        return Ok(v1::LabelColor::Unspecified);
    }
    let Some(color) = urushi_core::LabelColor::parse(name) else {
        bail!(
            "unknown color {name:?} (try red, orange, yellow, green, teal, blue, purple, pink, gray or none)"
        );
    };
    Ok(
        v1::LabelColor::try_from(urushi_proto::convert::color_to_proto(Some(color)))
            .unwrap_or_default(),
    )
}
