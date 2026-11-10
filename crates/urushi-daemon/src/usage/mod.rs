//! Plan usage of the agent CLI running in a pane (ADR 0011).
//!
//! Two questions, answered on the daemon's machine because that's where the agents and
//! their sign-ins are:
//! 1. **Which agent runs in this pane?** The pane's shell is the root of a process tree;
//!    the first descendant whose command is an agent CLI wins ([`process`]).
//! 2. **How much of its plan is used?** Each agent keeps that differently: Claude Code
//!    asks its usage endpoint with its own sign-in ([`claude`]); Codex logs its rate
//!    limits in its session files ([`codex`]). Cursor exposes nothing.
//!
//! Both are cached ([`UsageMonitor`]): the process table for a couple of seconds, usage
//! for minutes, and a rate-limited service is left alone for as long as it asks
//! ([`cache`]), so clients can poll cheaply. The slow parts sit behind [`UsageSource`]
//! so tests never touch the real keychain, network or home directory.

mod cache;
pub mod claude;
pub mod codex;
pub mod process;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use urushi_core::{Agent, Timestamp};

pub use cache::Failure;
pub use process::Process;

use crate::service::{blocking, lock};

/// How long the process table is reused.
const PROCESS_TTL: Duration = Duration::from_secs(2);

/// One rate-limit window, e.g. Claude's 5-hour or weekly limit.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// Short label: "5h", "Week", "Week · Opus".
    pub label: String,
    /// 0–100.
    pub used_percent: f64,
    /// When it resets (ms since the epoch), if known.
    pub resets_at_ms: Option<i64>,
}

/// An agent's plan and how much of it is used.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    /// The subscription, e.g. "Max 20x"; empty if unknown.
    pub plan: String,
    /// Windows, shortest first.
    pub windows: Vec<Window>,
}

/// What a client is told about a pane.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// The agent running in the pane, if any.
    pub agent: Option<Agent>,
    /// Its usage, or why it can't be read.
    pub usage: Result<Usage, String>,
    /// When the usage was read.
    pub fetched_at: Timestamp,
}

/// Facts about the machine's processes and agents: the process table, processes' exact
/// arguments and working directories, agents' open conversations and plan usage. The
/// system implementation runs `ps`, asks the OS (`sysinfo`), reads the agents' files and
/// asks their services; tests substitute fixed answers.
pub trait UsageSource: Send + Sync {
    /// Every process on this machine (empty where that can't be listed).
    fn processes(&self) -> Vec<Process>;
    /// The exact arguments and working directory of each of `pids` that can be read.
    /// Blocking.
    fn details(&self, pids: &[u32]) -> HashMap<u32, process::Details> {
        let _ = pids;
        HashMap::new()
    }
    /// The conversation `agent`, running as process `pid`, has open. Blocking.
    fn agent_conversation(&self, agent: Agent, pid: u32) -> crate::restore::Conversation {
        let _ = (agent, pid);
        crate::restore::Conversation::Unknown
    }
    /// Reads `agent`'s plan usage. Slow (network or files); called off the async
    /// runtime. Failures carry short, user-facing sentences.
    fn usage(&self, agent: Agent) -> Result<Usage, Failure>;
}

/// The real machine.
#[derive(Debug, Default)]
pub struct SystemUsage;

impl UsageSource for SystemUsage {
    fn processes(&self) -> Vec<Process> {
        crate::platform::process_table()
            .map(|text| process::parse_ps(&text))
            .unwrap_or_default()
    }

    fn details(&self, pids: &[u32]) -> HashMap<u32, process::Details> {
        process::details(pids)
    }

    fn agent_conversation(&self, agent: Agent, pid: u32) -> crate::restore::Conversation {
        match agent {
            Agent::Claude => crate::restore::sessions::claude_conversation(pid),
            Agent::Codex | Agent::Cursor => crate::restore::Conversation::Unknown,
        }
    }

    fn usage(&self, agent: Agent) -> Result<Usage, Failure> {
        match agent {
            Agent::Claude => claude::read(),
            Agent::Codex => Ok(codex::read(&codex::home().ok_or("no home directory")?)?),
            Agent::Cursor => Err("Cursor doesn't report its plan usage".into()),
        }
    }
}

type Cached<T> = Option<(Instant, T)>;

/// Answers "what agent runs in this pane, and how much of its plan is used", with caching.
pub struct UsageMonitor {
    source: Arc<dyn UsageSource>,
    processes: Mutex<Cached<Arc<Vec<Process>>>>,
    usage: Mutex<HashMap<Agent, cache::Entry>>,
}

impl UsageMonitor {
    /// A monitor reading from `source`.
    pub fn new(source: Arc<dyn UsageSource>) -> Self {
        Self {
            source,
            processes: Mutex::new(None),
            usage: Mutex::new(HashMap::new()),
        }
    }

    /// The agent running under `shell_pid` and its usage. `None` (no live process)
    /// means no agent.
    pub async fn report(&self, shell_pid: Option<u32>) -> Report {
        let agent = match shell_pid {
            Some(pid) => process::agent_under(&self.processes().await, pid),
            None => None,
        };
        let Some(agent) = agent else {
            return Report {
                agent: None,
                usage: Ok(Usage::default()),
                fetched_at: crate::service::now(),
            };
        };
        let (usage, fetched_at) = self.usage(agent).await;
        Report {
            agent: Some(agent),
            usage,
            fetched_at,
        }
    }

    /// Exact arguments and working directories of `pids` (see [`UsageSource::details`]).
    pub(crate) async fn details(&self, pids: Vec<u32>) -> HashMap<u32, process::Details> {
        let source = self.source.clone();
        blocking(move || Ok(source.details(&pids)))
            .await
            .unwrap_or_default()
    }

    /// The conversation an agent process has open (see
    /// [`UsageSource::agent_conversation`]).
    pub(crate) async fn agent_conversation(
        &self,
        agent: Agent,
        pid: u32,
    ) -> crate::restore::Conversation {
        let source = self.source.clone();
        blocking(move || Ok(source.agent_conversation(agent, pid)))
            .await
            .unwrap_or_default()
    }

    /// The process table, cached for a few seconds.
    pub(crate) async fn processes(&self) -> Arc<Vec<Process>> {
        self.cached_processes().await
    }

    /// A fresh process table (for restore points, which need the current foreground).
    pub(crate) async fn processes_now(&self) -> Arc<Vec<Process>> {
        *lock(&self.processes) = None;
        self.cached_processes().await
    }

    async fn cached_processes(&self) -> Arc<Vec<Process>> {
        if let Some((at, table)) = &*lock(&self.processes)
            && at.elapsed() < PROCESS_TTL
        {
            return table.clone();
        }
        let source = self.source.clone();
        let table = Arc::new(
            blocking(move || Ok(source.processes()))
                .await
                .unwrap_or_default(),
        );
        *lock(&self.processes) = Some((Instant::now(), table.clone()));
        table
    }

    async fn usage(&self, agent: Agent) -> (Result<Usage, String>, Timestamp) {
        if let Some(entry) = lock(&self.usage).get(&agent) {
            let now = Instant::now();
            if now < entry.next_read {
                return entry.shown(now);
            }
        }
        // Two clients asking at once may both read; that's harmless and rare.
        let source = self.source.clone();
        let result = blocking(move || Ok(source.usage(agent)))
            .await
            .unwrap_or_else(|e| Err(Failure::Unavailable(e.to_string())));
        if let Err(error) = &result {
            tracing::debug!(?agent, %error, "agent usage unavailable");
        }
        let fetched = crate::service::now();
        let now = Instant::now();
        let mut cached = lock(&self.usage);
        let entry = cache::record(cached.remove(&agent), result, now, fetched);
        let shown = entry.shown(now);
        cached.insert(agent, entry);
        shown
    }

    /// Makes every cached usage reading due, as if its wait had passed.
    #[cfg(test)]
    fn expire(&self) {
        let now = Instant::now();
        for entry in lock(&self.usage).values_mut() {
            entry.next_read = now;
        }
    }
}

/// `max` + `default_claude_max_20x` → "Max 20x"; `pro` → "Pro".
pub(crate) fn plan_label(plan: &str, tier: &str) -> String {
    let mut label = capitalize(plan.trim());
    if let Some(multiplier) = tier.rsplit('_').next().filter(|m| {
        m.len() > 1 && m.ends_with('x') && m[..m.len() - 1].bytes().all(|b| b.is_ascii_digit())
    }) {
        label = format!("{label} {multiplier}");
    }
    label
}

pub(crate) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect::<String>()
    })
}

/// Claude Code's config directory: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub(crate) fn claude_dir() -> Result<std::path::PathBuf, String> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| std::path::PathBuf::from(h).join(".claude"))
        })
        .ok_or_else(|| "no home directory".to_owned())
}

/// Claude Code's credentials file: `$CLAUDE_CONFIG_DIR/.credentials.json`, else
/// `~/.claude/.credentials.json`.
pub(crate) fn claude_credentials_file() -> Result<String, String> {
    let dir = claude_dir()?;
    std::fs::read_to_string(dir.join(".credentials.json"))
        .map_err(|_| "Claude isn't signed in (no stored credentials)".to_owned())
}

#[cfg(test)]
mod tests;
