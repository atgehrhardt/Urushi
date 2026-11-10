//! `urushi jira …`: signing the daemon in to Jira Cloud, and finding and importing
//! boards to follow (protocol 1.12). The API token is read from `URUSHI_JIRA_TOKEN` or
//! stdin, never from an argument, so it stays out of shell history and process lists.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use urushi_client::Client;

use crate::commands::{out, resolve};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum JiraCmd {
    /// Sign the daemon in to a Jira Cloud site. The API token (from
    /// id.atlassian.com → Security → API tokens) is read from `URUSHI_JIRA_TOKEN` or stdin.
    Connect {
        /// The site: "acme", "acme.atlassian.net" or its https address.
        #[arg(long)]
        site: String,
        /// The Atlassian account's email.
        #[arg(long)]
        email: String,
    },
    /// Show who the daemon is signed in to Jira as.
    Status,
    /// Forget the daemon's Jira sign-in (imported boards stay, but stop syncing).
    Disconnect,
    /// List the site's boards (whose name contains QUERY).
    Boards {
        /// Part of a board's name.
        #[arg(default_value = "")]
        query: String,
    },
    /// Import a Jira board as a board that stays in sync with it.
    Import {
        /// The Jira board's id (from `urushi jira boards`).
        board: u64,
    },
    /// List a Jira board's quick filters, or (given their ids) the issues all of them
    /// let through.
    Quick {
        /// The board (here, not in Jira).
        board: String,
        /// Quick filter ids (from the listing).
        ids: Vec<u64>,
    },
}

/// The API token: `URUSHI_JIRA_TOKEN`, or the first line of stdin.
fn token() -> Result<String> {
    if let Ok(token) = std::env::var("URUSHI_JIRA_TOKEN") {
        return Ok(token);
    }
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        let mut stderr = std::io::stderr().lock();
        write!(
            stderr,
            "API token (shown as you type; or set URUSHI_JIRA_TOKEN): "
        )?;
        stderr.flush()?;
    }
    let mut token = String::new();
    stdin.lock().read_line(&mut token)?;
    let token = token.trim().to_owned();
    if token.is_empty() {
        bail!("no API token given");
    }
    Ok(token)
}

pub(crate) async fn run(client: &Client, cmd: JiraCmd) -> Result<()> {
    match cmd {
        JiraCmd::Connect { site, email } => {
            let account = client.connect_jira(&site, &email, &token()?).await?;
            out(&format!(
                "signed in to {} as {} ({})",
                account.site, account.display_name, account.email
            ))
        }
        JiraCmd::Status => {
            let account = client.jira_account().await?;
            if account.connected {
                out(&format!(
                    "signed in to {} as {} ({})",
                    account.site, account.display_name, account.email
                ))
            } else {
                out("not signed in to Jira (sign in with `urushi jira connect`)")
            }
        }
        JiraCmd::Disconnect => {
            client.disconnect_jira().await?;
            out("signed out of Jira")
        }
        JiraCmd::Boards { query } => {
            let boards = client.list_jira_boards(&query).await?;
            if boards.is_empty() {
                return out("no boards found");
            }
            for b in boards {
                let imported = if b.imported_board_id.is_empty() {
                    String::new()
                } else {
                    format!("  (imported as {})", short(&b.imported_board_id))
                };
                out(&format!(
                    "{:>6}  {}  [{} {}]{imported}",
                    b.id,
                    b.name,
                    if b.scrum { "scrum" } else { "kanban" },
                    b.project_key
                ))?;
            }
            Ok(())
        }
        JiraCmd::Import { board } => {
            let board = client
                .import_jira_board(board)
                .await
                .context("importing the board")?;
            out(&format!(
                "board {} {} (its issues arrive as it syncs)",
                short(&board.id),
                board.name
            ))
        }
        JiraCmd::Quick { board, ids } => quick(client, &board, &ids).await,
    }
}

/// `urushi jira quick`: the quick filters, or the issues they let through.
async fn quick(client: &Client, board: &str, ids: &[u64]) -> Result<()> {
    let id = resolve(client, board).await?;
    let workspace = client.list_workspace(false).await?;
    let Some(board) = workspace.boards.iter().find(|b| b.id == id) else {
        bail!("no board {board}");
    };
    let Some(jira) = board.jira.as_deref() else {
        bail!("{} doesn't follow Jira", board.name);
    };
    if ids.is_empty() {
        if jira.quick_filters.is_empty() {
            return out("no quick filters");
        }
        for q in &jira.quick_filters {
            out(&format!("{:>6}  {}", q.id, q.name))?;
        }
        return Ok(());
    }
    let matching = client.query_jira_filter(&id, ids).await?;
    let issues = urushi_client::WorkspaceMirror::new(workspace.clone());
    for task in issues.all_tasks() {
        if let Some(issue) = task.issue().filter(|i| matching.contains(&i.id)) {
            out(&format!("{}  {}", issue.key, task.title()))?;
        }
    }
    Ok(())
}
