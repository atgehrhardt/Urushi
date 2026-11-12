//! `urushi project …` and the thread commands beyond pins and statuses: notes and
//! unread marks (ADR 0048).

use std::io::Read as _;

use anyhow::{Context, Result};
use clap::{Subcommand, ValueEnum};
use urushi_client::{Client, WorkspaceMirror};

use crate::boards::color_arg;
use crate::commands::{out, resolve, update_thread};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum ProjectCmd {
    /// Create a project.
    New {
        /// Name.
        name: String,
    },
    /// Rename a project.
    Rename {
        /// Project id.
        project: String,
        /// New name.
        name: String,
    },
    /// Delete a project. Its threads become ungrouped; nothing on disk changes.
    Rm {
        /// Project id.
        project: String,
    },
    /// Set the color of a project's sidebar section.
    Color {
        /// Project id.
        project: String,
        /// red, orange, yellow, green, teal, blue, purple, pink, gray, or none.
        color: String,
    },
    /// Move a project to a position in the list (0 is first).
    Move {
        /// Project id.
        project: String,
        /// New position, counted without the project (clamped to the end).
        index: u32,
    },
    /// Show or change a project's settings: its parent branch, which its new threads
    /// start from, and whether a parent-branch thread is kept at its top. With no
    /// options, prints them.
    Settings {
        /// Project id.
        project: String,
        /// The parent branch's repo (default: the one already set).
        #[arg(long)]
        repo: Option<String>,
        /// The parent branch: a local branch, or a remote one such as `origin/x`.
        #[arg(long)]
        branch: Option<String>,
        /// Remove the parent branch.
        #[arg(long, conflicts_with_all = ["repo", "branch"])]
        clear: bool,
        /// Keep a parent-branch thread at the project's top. Switching it off while it
        /// is open is refused: close it with `urushi thread rm`.
        #[arg(long, value_enum)]
        base_thread: Option<OnOff>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum OnOff {
    On,
    Off,
}

/// Runs a `urushi project …` command.
pub(crate) async fn project(client: &Client, cmd: ProjectCmd) -> Result<()> {
    match cmd {
        ProjectCmd::New { name } => {
            let p = client.create_project(&name).await?;
            out(&format!("project {} {}", short(&p.id), p.name))
        }
        ProjectCmd::Rename { project, name } => Ok(client
            .rename_project(&resolve(client, &project).await?, &name)
            .await?),
        ProjectCmd::Rm { project } => Ok(client
            .delete_project(&resolve(client, &project).await?)
            .await?),
        ProjectCmd::Color { project, color } => Ok(client
            .set_project_color(&resolve(client, &project).await?, color_arg(&color)?)
            .await?),
        ProjectCmd::Move { project, index } => Ok(client
            .move_project(&resolve(client, &project).await?, index)
            .await?),
        ProjectCmd::Settings {
            project,
            repo,
            branch,
            clear,
            base_thread,
        } => {
            let change = crate::project_settings::Change {
                repo,
                branch,
                clear,
                base_thread: base_thread.map(|b| matches!(b, OnOff::On)),
            };
            crate::project_settings::run(client, &project, change).await
        }
    }
}

/// `urushi thread notes`: prints a thread's notes, or replaces (`set`, "-" for standard
/// input) or clears them.
pub(crate) async fn notes(
    client: &Client,
    thread: &str,
    set: Option<String>,
    clear: bool,
) -> Result<()> {
    let notes = match (set, clear) {
        (_, true) => String::new(),
        (Some(text), false) if text == "-" => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("reading the notes from standard input")?;
            text
        }
        (Some(text), false) => text,
        (None, false) => {
            let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
            let id = resolve(client, thread).await?;
            let thread = mirror.thread(&id).context("no such thread")?;
            return if thread.notes.is_empty() {
                Ok(())
            } else {
                out(&thread.notes)
            };
        }
    };
    update_thread(client, thread, |r| r.notes = Some(notes)).await
}
