//! A machine whose agents' conversations are known, and a stand-in `claude`, for tests
//! of panes coming back from their restore points (restarts, unshelving).

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use urushi_core::{Agent, AgentSessionId};
use urushi_daemon::DaemonOptions;
use urushi_daemon::restore::Conversation;
use urushi_daemon::tailscale::StaticTailscale;
use urushi_daemon::usage::process::Details;
use urushi_daemon::usage::{Failure, Process, SystemUsage, Usage, UsageSource};

/// The real machine, except that every Claude has session `restored-session-1` open.
pub struct Machine;

impl UsageSource for Machine {
    fn processes(&self) -> Vec<Process> {
        SystemUsage.processes()
    }
    fn details(&self, pids: &[u32]) -> HashMap<u32, Details> {
        SystemUsage.details(pids)
    }
    fn agent_conversation(&self, agent: Agent, _pid: u32) -> Conversation {
        match agent {
            Agent::Claude => {
                Conversation::Saved(AgentSessionId::new("restored-session-1").unwrap())
            }
            _ => Conversation::Unknown,
        }
    }
    fn usage(&self, _agent: Agent) -> Result<Usage, Failure> {
        Err("not here".into())
    }
}

/// A stand-in `claude` that prints its arguments (after Urushi's hook options) and keeps
/// running, like the real one.
pub fn fake_claude(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("claude");
    std::fs::write(
        &path,
        "#!/bin/sh\nwhile [ \"$1\" = --settings ]; do shift 2; done\necho \"FAKE claude ARGS:$*:END\"\nsleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A daemon in `home` that finds agents in `bin` and runs on [`Machine`].
pub fn options(home: &Path, bin: &Path) -> DaemonOptions {
    let mut options = DaemonOptions::new(home);
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.agents = Arc::new(urushi_daemon::agents::AgentLocator::in_dirs(vec![
        bin.to_path_buf(),
    ]));
    options.usage = Arc::new(Machine);
    options
}
