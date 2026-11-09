//! Bringing panes back after the daemon restarts, from their restore points
//! ([`crate::restore`]).
//!
//! Every [`SAVE_INTERVAL`], and when the daemon shuts down, each pane's restore point is
//! brought up to date: its size, its working directory, the agent running in it and the
//! conversation it has open, or else the job in the terminal's foreground, and its
//! recent output. When the daemon starts, every pane of a ready thread is started from
//! it: a fresh shell in the same directory, below the old output, and then the agent
//! resuming its conversation or the job running again, typed into the shell like
//! starting an agent (ADR 0009). Panes without a restore point start as plain shells in
//! their worktree.

use std::collections::HashSet;
use std::time::Duration;

use urushi_core::{PaneId, Thread, ThreadState};
use urushi_mux::replay::to_ansi;
use urushi_mux::{AlacrittyEmulator, Emulator as _, TermSize};

use super::{Service, blocking, lock};
use crate::restore::{AgentRecord, AgentStart, SCREEN_LINES};
use crate::usage::process::{agent_process_under, foreground_job};

/// How often restore points are brought up to date. A crash or power cut loses at most
/// this much.
const SAVE_INTERVAL: Duration = Duration::from_secs(15);

/// How long a new shell must stay quiet after printing before it counts as ready.
const READY_QUIET: Duration = Duration::from_millis(500);
/// Longest to wait for a new shell to be ready before typing anyway.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// What a restored pane starts in its new shell.
enum Restart {
    /// The agent, resuming its conversation.
    Agent(AgentRecord),
    /// The job that was in the foreground, as a command line.
    Job(String),
}

/// Shown between a restored pane's old output and its new shell.
const SEPARATOR: &str = "\x1b[0;2m── restored after the daemon restarted ──\x1b[0m\r\n";

impl Service {
    /// Starts every pane of every ready thread again, from its restore point.
    pub(super) fn restore_sessions(&self) {
        let work: Vec<(PaneId, Thread, Option<String>)> = {
            let state = lock(&self.state);
            state
                .tabs
                .iter()
                .filter_map(|tab| {
                    let record = state.threads.iter().find(|r| r.thread.id == tab.thread())?;
                    if record.thread.state != ThreadState::Ready {
                        return None;
                    }
                    let shell = state.effective_settings(record.thread.repo).default_shell;
                    Some(
                        tab.layout()
                            .panes()
                            .into_iter()
                            .map(move |p| (p, record.thread.clone(), shell.clone())),
                    )
                })
                .flatten()
                .collect()
        };
        let mut restarts = Vec::new();
        for (pane, thread, shell) in work {
            let record = self.restore.load(pane).unwrap_or_default();
            let size = if record.cols > 0 && record.rows > 0 {
                TermSize::new(record.cols, record.rows)
            } else {
                TermSize::DEFAULT
            };
            let mut emulator = AlacrittyEmulator::new(size);
            // An agent redraws its own conversation when it resumes.
            if record.agent.is_none() && !record.screen.is_empty() {
                emulator.advance(record.screen.as_bytes());
                emulator.advance(SEPARATOR.as_bytes());
            }
            let cwd = record.cwd.clone();
            match self.spawn_session_in(pane, &thread, shell.clone(), cwd, emulator, Vec::new()) {
                Ok(()) => {
                    if let Some(agent) = record.agent.clone() {
                        restarts.push((pane, shell, Restart::Agent(agent)));
                    } else if let Some(line) = urushi_core::shell::job_command_line(&record.job) {
                        restarts.push((pane, shell, Restart::Job(line)));
                    }
                    lock(&self.restore_points).insert(pane, (0, record));
                }
                Err(e) => tracing::error!(%pane, error = %e, "failed to restore pane"),
            }
        }
        if restarts.is_empty() {
            return;
        }
        for (pane, shell, restart) in restarts {
            let me = self.me.clone();
            tokio::spawn(async move {
                let Some(service) = me.upgrade() else { return };
                service.wait_for_prompt(pane).await;
                match restart {
                    Restart::Agent(agent) => service.resume_agent(pane, shell, agent).await,
                    Restart::Job(line) => {
                        if let Err(e) = service.type_command(pane, &line).await {
                            tracing::warn!(%pane, error = %e, "couldn't run the job again");
                        }
                    }
                }
            });
        }
    }

    /// Waits until `pane`'s new shell is ready for input: it has printed something (its
    /// prompt) and then been quiet for [`READY_QUIET`], or [`READY_TIMEOUT`] has passed.
    /// Some shell setups discard what was typed while they started, so typing a command
    /// straight after spawning can be lost.
    async fn wait_for_prompt(&self, pane: PaneId) {
        let Ok(handle) = self.session(pane) else {
            return;
        };
        let mut status = handle.status();
        let started = status.borrow_and_update().version;
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        let mut printed = false;
        loop {
            let next = tokio::time::timeout(READY_QUIET, status.changed());
            match tokio::time::timeout_at(deadline, next).await {
                // Out of patience, or the pane closed.
                Err(_) | Ok(Ok(Err(_))) => return,
                // Quiet for a while after printing: the prompt is up.
                Ok(Err(_)) if printed => return,
                Ok(Err(_)) => {}
                Ok(Ok(Ok(()))) => printed |= status.borrow_and_update().version != started,
            }
        }
    }

    /// Types the command that starts `record`'s agent again into `pane`'s new shell:
    /// resuming its conversation, or afresh if it had none. If the agent is no longer
    /// installed, the pane stays a plain shell.
    async fn resume_agent(&self, pane: PaneId, shell: Option<String>, record: AgentRecord) {
        let line = match record.start() {
            AgentStart::Fresh => self.agent_command_line(record.agent, shell).await,
            AgentStart::Resume(session) => {
                self.agent_resume_line(record.agent, shell, session).await
            }
        };
        let result = match line {
            Ok(line) => self.type_command(pane, &line).await,
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            tracing::warn!(%pane, agent = %record.agent, error = %e, "couldn't resume agent");
        }
    }

    /// Saves restore points every [`SAVE_INTERVAL`] while the daemon runs.
    pub(super) fn keep_restore_points(&self) {
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SAVE_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tick.tick().await;
            loop {
                tick.tick().await;
                let Some(service) = me.upgrade() else { break };
                service.save_restore_points().await;
            }
        });
    }

    /// Brings every pane's restore point up to date, and deletes those of panes that no
    /// longer exist. Output is read again only from panes whose screen changed; a
    /// full-screen application's screen (`vim`, `less`) keeps the output from before it.
    pub async fn save_restore_points(&self) {
        let sessions: Vec<_> = lock(&self.sessions)
            .iter()
            .map(|(pane, handle)| (*pane, handle.clone()))
            .collect();
        let processes = self.usage.processes_now().await;
        // What runs in each pane: an agent, or else the job in the foreground.
        let running: Vec<_> = sessions
            .iter()
            .map(|(_, handle)| {
                let pid = handle.pid();
                let agent = pid.and_then(|pid| agent_process_under(&processes, pid));
                let job = match (pid, agent) {
                    (Some(pid), None) => foreground_job(&processes, pid),
                    _ => Vec::new(),
                };
                (pid, agent, job)
            })
            .collect();
        let pids = running
            .iter()
            .flat_map(|(pid, _, job)| pid.iter().copied().chain(job.iter().map(|p| p.pid)))
            .collect();
        let details = self.usage.details(pids).await;
        for ((pane, handle), (pid, agent, job)) in sessions.into_iter().zip(running) {
            let agent = match agent {
                Some((agent, agent_pid)) => Some(AgentRecord {
                    agent,
                    conversation: self.usage.agent_conversation(agent, agent_pid).await,
                }),
                None => None,
            };
            let job_argv: Vec<Vec<String>> = job
                .iter()
                .map(|p| {
                    details
                        .get(&p.pid)
                        .map(|d| d.argv.clone())
                        .filter(|argv| !argv.is_empty())
                        // Without exact arguments, the table's (split at spaces).
                        .unwrap_or_else(|| p.args.split_whitespace().map(String::from).collect())
                })
                .collect();
            // A job may run somewhere else than the shell's directory (`cd x && make`).
            let cwd_pid = job.first().map_or(pid, |p| Some(p.pid));
            let previous = lock(&self.restore_points).get(&pane).cloned();
            let (mut seen, mut record) = previous.clone().unwrap_or_default();
            let version = handle.status().borrow().version;
            if previous.is_none() || version != seen {
                let Ok(snapshot) = handle.snapshot(SCREEN_LINES).await else {
                    continue; // The pane just closed.
                };
                record.cols = snapshot.size.cols;
                record.rows = snapshot.size.rows;
                if agent.is_some() {
                    record.screen.clear();
                } else if !snapshot.alt_screen {
                    record.screen = to_ansi(&snapshot.lines);
                }
                seen = version;
            }
            if let Some(cwd) = cwd_pid.and_then(|p| details.get(&p)?.cwd.clone()) {
                record.cwd = Some(cwd);
            }
            record.agent = agent;
            record.job = job_argv;
            if previous.as_ref().map(|(_, r)| r) != Some(&record) {
                let (store, saved) = (self.restore.clone(), record.clone());
                let result = blocking(move || {
                    store
                        .save(pane, &saved)
                        .map_err(|e| super::ServiceError::Internal(e.to_string()))
                })
                .await;
                if let Err(e) = result {
                    tracing::warn!(%pane, error = %e, "couldn't save restore point");
                }
            }
            lock(&self.restore_points).insert(pane, (seen, record));
        }
        let keep: HashSet<PaneId> = lock(&self.state)
            .tabs
            .iter()
            .flat_map(|t| t.layout().panes())
            .collect();
        lock(&self.restore_points).retain(|pane, _| keep.contains(pane));
        let store = self.restore.clone();
        let _ = blocking(move || {
            store.retain(&keep);
            Ok(())
        })
        .await;
    }
}
