//! `urushi jira …`: signing the daemon in to Jira Cloud sites (several since protocol
//! 1.29, ADR 0051), and finding and importing boards to follow (protocol 1.12). The API
//! token is read from `URUSHI_JIRA_TOKEN` or stdin, never from an argument, so it stays
//! out of shell history and process lists. `--account` picks a sign-in by its key, its
//! email or its site; it can be left out while there is only one.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use urushi_client::Client;

use crate::commands::{out, resolve};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum JiraCmd {
    /// Sign the daemon in to a Jira Cloud site, beside any other sign-ins (the same site
    /// and email again replaces that one). The API token (from id.atlassian.com →
    /// Security → API tokens) is read from `URUSHI_JIRA_TOKEN` or stdin.
    Connect {
        /// The site: "acme", "acme.atlassian.net" or its https address.
        #[arg(long)]
        site: String,
        /// The Atlassian account's email.
        #[arg(long)]
        email: String,
    },
    /// Show who the daemon is signed in to Jira as, on each site.
    Status,
    /// Forget one of the daemon's Jira sign-ins (its boards stay, but stop syncing).
    Disconnect {
        /// The sign-in: its key, email or site (needed when there are several).
        #[arg(long)]
        account: Option<String>,
    },
    /// List a site's boards (whose name contains QUERY).
    Boards {
        /// Part of a board's name.
        #[arg(default_value = "")]
        query: String,
        /// The sign-in whose site to list: its key, email or site.
        #[arg(long)]
        account: Option<String>,
    },
    /// Import a Jira board as a board that stays in sync with it.
    Import {
        /// The Jira board's id (from `urushi jira boards`).
        board: u64,
        /// The sign-in to import and sync it with: its key, email or site.
        #[arg(long)]
        account: Option<String>,
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
            let accounts = signed_in(client).await?;
            if accounts.is_empty() {
                return out("not signed in to Jira (sign in with `urushi jira connect`)");
            }
            for a in accounts {
                out(&format!(
                    "signed in to {} as {} ({})  [{}]",
                    a.site, a.display_name, a.email, a.key
                ))?;
            }
            Ok(())
        }
        JiraCmd::Disconnect { account } => {
            let account = pick(client, account.as_deref()).await?;
            client.disconnect_jira(&account).await?;
            out("signed out of Jira")
        }
        JiraCmd::Boards { query, account } => {
            let account = pick(client, account.as_deref()).await?;
            let boards = client.list_jira_boards(&account, &query).await?;
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
        JiraCmd::Import { board, account } => {
            let account = pick(client, account.as_deref()).await?;
            let board = client
                .import_jira_board(&account, board)
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

/// The daemon's sign-ins: every one from a 1.29 daemon, else the one it has.
async fn signed_in(client: &Client) -> Result<Vec<urushi_client::v1::JiraAccount>> {
    if client.welcome().protocol_minor >= JIRA_ACCOUNTS_SINCE_MINOR {
        return Ok(client.jira_accounts().await?);
    }
    let account = client.jira_account().await?;
    Ok(if account.connected {
        vec![account]
    } else {
        Vec::new()
    })
}

/// Daemons from this protocol minor keep several sign-ins.
const JIRA_ACCOUNTS_SINCE_MINOR: u32 = 29;

/// The key of the sign-in `wanted` names (its key, email or site), or the only one;
/// empty (the daemon's first) when nothing is asked of an older daemon.
async fn pick(client: &Client, wanted: Option<&str>) -> Result<String> {
    if client.welcome().protocol_minor < JIRA_ACCOUNTS_SINCE_MINOR {
        if wanted.is_some() {
            bail!("this daemon keeps one Jira sign-in; restart it to keep several");
        }
        return Ok(String::new());
    }
    let accounts = client.jira_accounts().await?;
    choose(&accounts, wanted)
}

/// [`pick`]'s choice among `accounts`.
fn choose(accounts: &[urushi_client::v1::JiraAccount], wanted: Option<&str>) -> Result<String> {
    let site =
        |a: &urushi_client::v1::JiraAccount| a.site.trim_start_matches("https://").to_owned();
    let found: Vec<&urushi_client::v1::JiraAccount> = match wanted {
        None => accounts.iter().collect(),
        Some(w) => {
            let w = w.trim().trim_start_matches("https://").to_lowercase();
            accounts
                .iter()
                .filter(|a| {
                    a.key == w
                        || a.email.to_lowercase() == w
                        || site(a) == w
                        || site(a).split('.').next() == Some(w.as_str())
                })
                .collect()
        }
    };
    match found[..] {
        [one] => Ok(one.key.clone()),
        [] if accounts.is_empty() => {
            bail!("not signed in to Jira (sign in with `urushi jira connect`)")
        }
        [] => bail!(
            "no Jira sign-in {}; there are: {}",
            wanted.unwrap_or_default(),
            keys(accounts)
        ),
        _ => bail!(
            "which Jira sign-in? Pass --account with one of: {}",
            keys(&found.into_iter().cloned().collect::<Vec<_>>())
        ),
    }
}

fn keys(accounts: &[urushi_client::v1::JiraAccount]) -> String {
    accounts
        .iter()
        .map(|a| a.key.as_str())
        .collect::<Vec<_>>()
        .join(", ")
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

#[cfg(test)]
mod tests {
    use super::choose;
    use urushi_client::v1::JiraAccount;

    fn account(site: &str, email: &str) -> JiraAccount {
        JiraAccount {
            connected: true,
            site: format!("https://{site}"),
            email: email.into(),
            key: format!("{site}/{email}"),
            ..Default::default()
        }
    }

    #[test]
    fn a_sign_in_is_chosen_by_key_email_or_site() {
        let accounts = [
            account("acme.atlassian.net", "ada@acme.com"),
            account("globex.atlassian.net", "ada@globex.com"),
        ];
        for wanted in [
            "globex",
            "globex.atlassian.net",
            "https://globex.atlassian.net",
            "ADA@globex.com",
            "globex.atlassian.net/ada@globex.com",
        ] {
            assert_eq!(
                choose(&accounts, Some(wanted)).unwrap(),
                "globex.atlassian.net/ada@globex.com",
                "{wanted}"
            );
        }
        assert!(choose(&accounts, None).is_err(), "two: which one?");
        assert!(choose(&accounts, Some("initech")).is_err());
        assert_eq!(
            choose(&accounts[..1], None).unwrap(),
            "acme.atlassian.net/ada@acme.com"
        );
        assert!(choose(&[], None).is_err());
    }
}
