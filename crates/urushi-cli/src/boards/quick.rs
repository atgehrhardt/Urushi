//! `urushi board quick …`: a board's quick filters (protocol 1.23, ADR 0037). On a Jira
//! board they are Jira's (JQL), and the daemon writes them to Jira, or keeps a new one
//! itself when Jira won't take it; on a board of Urushi's own they filter by search,
//! kind and tag.

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use urushi_client::{Client, QUICK_FILTERS_SINCE_MINOR, jira_filter, kept_filter, v1};

use super::kind_arg;
use crate::commands::{out, resolve};
use crate::render::short;

#[derive(Subcommand)]
pub(crate) enum QuickCmd {
    /// List a board's quick filters.
    Ls {
        /// Board id.
        board: String,
    },
    /// Add a quick filter: JQL on a Jira board, otherwise a search, kinds and tags.
    Add {
        /// Board id.
        board: String,
        /// Name.
        name: String,
        #[command(flatten)]
        rule: RuleArgs,
    },
    /// Rename a quick filter and/or change what it lets through.
    Edit {
        /// Board id.
        board: String,
        /// The filter: Jira's number, or the name or id of one the board keeps.
        filter: String,
        /// New name.
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        rule: RuleArgs,
    },
    /// Remove a quick filter (a Jira board's own is deleted in Jira).
    Rm {
        /// Board id.
        board: String,
        /// The filter: Jira's number, or the name or id of one the board keeps.
        filter: String,
    },
}

/// What a quick filter lets through.
#[derive(clap::Args)]
pub(crate) struct RuleArgs {
    /// JQL (a Jira board).
    #[arg(long)]
    jql: Option<String>,
    /// Text the title or notes must contain (a board of Urushi's own).
    #[arg(long)]
    search: Option<String>,
    /// A kind it lets through (task, story, bug, epic, subtask); repeatable.
    #[arg(long = "kind")]
    kinds: Vec<String>,
    /// A tag it lets through; repeatable.
    #[arg(long = "tag")]
    tags: Vec<String>,
}

impl RuleArgs {
    /// The rule, or `None` when no criterion was given.
    fn rule(self) -> Result<Option<v1::QuickFilterRule>> {
        if self.jql.is_none()
            && self.search.is_none()
            && self.kinds.is_empty()
            && self.tags.is_empty()
        {
            return Ok(None);
        }
        Ok(Some(v1::QuickFilterRule {
            search: self.search.unwrap_or_default(),
            kinds: self
                .kinds
                .iter()
                .map(|k| kind_arg(k))
                .collect::<Result<_>>()?,
            tags: self.tags,
            jql: self.jql.unwrap_or_default(),
        }))
    }
}

pub(crate) async fn run(client: &Client, cmd: QuickCmd) -> Result<()> {
    if client.welcome().protocol_minor < QUICK_FILTERS_SINCE_MINOR {
        bail!(
            "the daemon is too old for quick filters (it needs protocol 1.{QUICK_FILTERS_SINCE_MINOR})"
        );
    }
    match cmd {
        QuickCmd::Ls { board } => list(&board_of(client, &board).await?),
        QuickCmd::Add { board, name, rule } => {
            let board = resolve(client, &board).await?;
            let Some(rule) = rule.rule()? else {
                bail!("say what it filters by: --jql, or --search, --kind and --tag");
            };
            let saved = client.create_quick_filter(&board, &name, rule).await?;
            match saved.filter.and_then(|f| f.filter) {
                Some(v1::quick_filter_ref::Filter::JiraId(id)) => {
                    out(&format!("quick filter {id} {name} (in Jira)"))
                }
                Some(v1::quick_filter_ref::Filter::Id(id)) if !saved.kept_here.is_empty() => {
                    out(&format!(
                        "quick filter {} {name} (kept here: {})",
                        short(&id),
                        saved.kept_here
                    ))
                }
                Some(v1::quick_filter_ref::Filter::Id(id)) => {
                    out(&format!("quick filter {} {name}", short(&id)))
                }
                None => Ok(()),
            }
        }
        QuickCmd::Edit {
            board,
            filter,
            name,
            rule,
        } => {
            let b = board_of(client, &board).await?;
            let filter = filter_ref(&b, &filter)?;
            let rule = rule.rule()?;
            if name.is_none() && rule.is_none() {
                bail!("nothing to change: give --name, --jql, --search, --kind or --tag");
            }
            Ok(client
                .update_quick_filter(&b.id, filter, name.as_deref().unwrap_or_default(), rule)
                .await?)
        }
        QuickCmd::Rm { board, filter } => {
            let b = board_of(client, &board).await?;
            Ok(client
                .delete_quick_filter(&b.id, filter_ref(&b, &filter)?)
                .await?)
        }
    }
}

async fn board_of(client: &Client, board: &str) -> Result<v1::Board> {
    let id = resolve(client, board).await?;
    client
        .list_workspace(false)
        .await?
        .boards
        .into_iter()
        .find(|b| b.id == id)
        .with_context(|| format!("no board {board}"))
}

/// The filter `arg` names: a number is one of Jira's, anything else one the board
/// keeps, by name (ignoring case) or by id (or its start or end).
fn filter_ref(board: &v1::Board, arg: &str) -> Result<v1::QuickFilterRef> {
    if let Ok(id) = arg.parse::<u64>() {
        return Ok(jira_filter(id));
    }
    let mut found = board.quick_filters.iter().filter(|q| {
        q.name.eq_ignore_ascii_case(arg) || q.id.starts_with(arg) || q.id.ends_with(arg)
    });
    match (found.next(), found.next()) {
        (Some(q), None) => Ok(kept_filter(&q.id)),
        (None, _) => bail!("no quick filter {arg:?} on {}", board.name),
        (Some(_), Some(_)) => bail!("{arg:?} names more than one quick filter"),
    }
}

fn list(board: &v1::Board) -> Result<()> {
    let jira = board
        .jira
        .as_deref()
        .map(|j| j.quick_filters.as_slice())
        .unwrap_or_default();
    if jira.is_empty() && board.quick_filters.is_empty() {
        return out("no quick filters");
    }
    for q in jira {
        out(&format!("{:>8}  {}  [Jira] {}", q.id, q.name, q.jql))?;
    }
    for q in &board.quick_filters {
        let rule = q.rule.clone().unwrap_or_default();
        let what = if rule.jql.is_empty() {
            describe(&rule)
        } else {
            format!("[kept here] {}", rule.jql)
        };
        out(&format!("{:>8}  {}  {what}", short(&q.id), q.name))?;
    }
    Ok(())
}

/// A rule of search, kinds and tags, in words.
fn describe(rule: &v1::QuickFilterRule) -> String {
    let mut parts = Vec::new();
    if !rule.search.is_empty() {
        parts.push(format!("search {:?}", rule.search));
    }
    let kinds: Vec<&str> = rule
        .kinds
        .iter()
        .filter_map(|k| urushi_proto::convert::kind_from_proto(*k).map(urushi_core::TaskKind::name))
        .collect();
    if !kinds.is_empty() {
        parts.push(format!("kind {}", kinds.join("|")));
    }
    if !rule.tags.is_empty() {
        parts.push(format!("tag {}", rule.tags.join("|")));
    }
    parts.join(", ")
}
