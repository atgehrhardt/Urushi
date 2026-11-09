//! `urushi board attach` and `urushi board attachment get|rm`: files attached to tasks
//! (protocol 1.8, ADR 0016).

use anyhow::{Context, Result};
use clap::Subcommand;
use urushi_client::{Client, WorkspaceMirror};

use crate::commands::{out, resolve};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum AttachmentCmd {
    /// Save an attachment's file (or an image's thumbnail, as raw RGBA).
    Get {
        /// Attachment id.
        attachment: String,
        /// The image's thumbnail instead of the file.
        #[arg(long)]
        thumbnail: bool,
        /// Where to save it (default: the file's name, in the current directory).
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Delete an attachment.
    Rm {
        /// Attachment id.
        attachment: String,
    },
}

/// Attaches the file at `path` to a task or subtask.
pub(super) async fn attach(client: &Client, task: &str, path: &str) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    let a = client
        .add_attachment(&resolve(client, task).await?, path, bytes)
        .await?;
    out(&format!(
        "attachment {} {} ({}, {} bytes)",
        short(&a.id),
        a.name,
        a.media_type,
        a.size
    ))
}

pub(super) async fn run(client: &Client, cmd: AttachmentCmd) -> Result<()> {
    match cmd {
        AttachmentCmd::Get {
            attachment,
            thumbnail,
            output,
        } => {
            let id = resolve_attachment(client, &attachment).await?;
            let data = client.get_attachment(&id.0, thumbnail).await?;
            let path = output.unwrap_or_else(|| {
                if thumbnail {
                    format!("{}.rgba", id.1)
                } else {
                    id.1.clone()
                }
            });
            std::fs::write(&path, &data.data).with_context(|| format!("writing {path}"))?;
            if thumbnail {
                out(&format!("{path}: {}×{} RGBA", data.width, data.height))
            } else {
                out(&format!("{path}: {} bytes", data.data.len()))
            }
        }
        AttachmentCmd::Rm { attachment } => {
            let id = resolve_attachment(client, &attachment).await?;
            Ok(client.delete_attachment(&id.0).await?)
        }
    }
}

/// An attachment's full id and file name, from its id or a unique part of it.
async fn resolve_attachment(client: &Client, id: &str) -> Result<(String, String)> {
    let id = resolve(client, id).await?;
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    let (_, a) = mirror.attachment(&id).context("no such attachment")?;
    Ok((a.id.clone(), a.name.clone()))
}
