//! What runs in a pane, from the machine's process table: the agent CLI under its
//! shell, and the job in the terminal's foreground.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use urushi_core::Agent;

/// What the OS says about one process beyond the process table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    /// Its exact arguments, the program first.
    pub argv: Vec<String>,
    /// Its working directory.
    pub cwd: Option<PathBuf>,
}

/// Arguments as a process's memory holds them, cut at the first control character. A
/// program may overwrite the end of its own arguments (BSD `yes` turns its last NUL
/// into a newline), so the OS reads the next strings (its environment) into that one.
fn clean_argv<S: AsRef<str>>(argv: impl Iterator<Item = S>) -> Vec<String> {
    argv.map(|a| {
        let a = a.as_ref();
        a.split(char::is_control).next().unwrap_or(a).to_owned()
    })
    .collect()
}

/// [`Details`] of each of `pids` the OS will describe (the user's own processes).
pub fn details(pids: &[u32]) -> HashMap<u32, Details> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    if pids.is_empty() {
        return HashMap::new();
    }
    let wanted: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&wanted),
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
    wanted
        .iter()
        .filter_map(|pid| {
            let process = system.process(*pid)?;
            let details = Details {
                argv: clean_argv(process.cmd().iter().map(|a| a.to_string_lossy())),
                cwd: process.cwd().map(PathBuf::from),
            };
            Some((pid.as_u32(), details))
        })
        .collect()
}

/// One process: its id, its parent, its process group, its terminal's foreground
/// process group, and its command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    /// Process id.
    pub pid: u32,
    /// Parent process id.
    pub ppid: u32,
    /// Process group id.
    pub pgid: u32,
    /// The foreground process group of its terminal (0 without one).
    pub tpgid: u32,
    /// The command line, arguments separated by spaces (so not exact: see
    /// [`super::UsageSource::details`] for the real arguments).
    pub args: String,
}

/// The `ps -o` columns [`parse_ps`] reads.
pub const PS_COLUMNS: &str = "pid=,ppid=,pgid=,tpgid=,args=";

/// Parses `ps -A -o pid=,ppid=,pgid=,tpgid=,args=` output. Malformed lines are skipped.
pub fn parse_ps(text: &str) -> Vec<Process> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let mut number = || parts.next()?.parse::<i64>().ok();
            let (id, parent, group, foreground) = (number()?, number()?, number()?, number()?);
            let id_or_0 = |n: i64| u32::try_from(n).unwrap_or(0);
            Some(Process {
                pid: u32::try_from(id).ok()?,
                ppid: id_or_0(parent),
                pgid: id_or_0(group),
                // `ps` prints -1 (Linux) or 0 (macOS) for "no terminal".
                tpgid: id_or_0(foreground),
                args: parts.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

/// The job in the foreground of the terminal whose shell is `shell`: the shell's
/// children in the terminal's foreground process group, oldest first (several for a
/// pipeline). Empty while the shell itself is in the foreground (at its prompt), and
/// for background jobs.
pub fn foreground_job(processes: &[Process], shell: u32) -> Vec<&Process> {
    let Some(sh) = processes.iter().find(|p| p.pid == shell) else {
        return Vec::new();
    };
    if sh.tpgid == 0 || sh.tpgid == sh.pgid {
        return Vec::new();
    }
    let mut job: Vec<&Process> = processes
        .iter()
        .filter(|p| p.ppid == shell && p.pgid == sh.tpgid && p.pid != shell)
        .collect();
    job.sort_by_key(|p| p.pid);
    job
}

/// Programs that run a script named by their first argument.
const INTERPRETERS: [&str; 9] = [
    "node", "bun", "deno", "sh", "bash", "zsh", "python", "python3", "env",
];

/// The agent a command line runs, if any: `claude …`, `/path/to/codex …`, or an
/// interpreter running such a script (`node /…/bin/codex.js`).
pub fn agent_of(args: &str) -> Option<Agent> {
    let mut words = args.split_whitespace();
    let first = name(words.next()?);
    let program = if INTERPRETERS.contains(&first.as_str()) {
        words.find(|w| !w.starts_with('-')).map(name)?
    } else {
        first
    };
    let program = program
        .trim_end_matches(".js")
        .trim_end_matches(".mjs")
        .trim_end_matches(".cjs");
    Agent::ALL
        .into_iter()
        .find(|a| a.program() == program)
        // Cursor's launcher runs `node …/cursor-agent/versions/<v>/index.js`.
        .or_else(|| args.contains("/cursor-agent/").then_some(Agent::Cursor))
}

/// A word's file name, without a login shell's leading dash.
fn name(word: &str) -> String {
    word.rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .trim_start_matches('-')
        .to_owned()
}

/// The agent nearest to `root` among it and its descendants.
pub fn agent_under(processes: &[Process], root: u32) -> Option<Agent> {
    agent_process_under(processes, root).map(|(agent, _)| agent)
}

/// The agent nearest to `root` among it and its descendants, and its process id (the
/// outermost process running it, e.g. `node` rather than its native helper).
pub fn agent_process_under(processes: &[Process], root: u32) -> Option<(Agent, u32)> {
    let mut children: HashMap<u32, Vec<&Process>> = HashMap::new();
    for p in processes {
        if p.pid != p.ppid {
            children.entry(p.ppid).or_default().push(p);
        }
    }
    if let Some(agent) = processes
        .iter()
        .find(|p| p.pid == root)
        .and_then(|p| agent_of(&p.args))
    {
        return Some((agent, root));
    }
    let mut queue: VecDeque<u32> = VecDeque::from([root]);
    let mut seen = 0usize;
    while let Some(pid) = queue.pop_front() {
        for child in children.get(&pid).into_iter().flatten() {
            if let Some(agent) = agent_of(&child.args) {
                return Some((agent, child.pid));
            }
            queue.push_back(child.pid);
        }
        // A corrupt table could contain a cycle; a pane never has this many processes.
        seen += 1;
        if seen > processes.len() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_name_agents() {
        assert_eq!(agent_of("claude"), Some(Agent::Claude));
        assert_eq!(
            agent_of("/Users/me/.local/bin/claude --dangerously-skip-permissions"),
            Some(Agent::Claude)
        );
        assert_eq!(
            agent_of("node /opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            Some(Agent::Codex)
        );
        assert_eq!(
            agent_of("node --no-warnings /Users/me/.local/share/cursor-agent/versions/1/index.js"),
            Some(Agent::Cursor)
        );
        assert_eq!(agent_of("/bin/sh ./claude"), Some(Agent::Claude));
        assert_eq!(agent_of("-zsh"), None);
        assert_eq!(agent_of("vim claude.md"), None);
        assert_eq!(agent_of("node server.js"), None);
        assert_eq!(agent_of(""), None);
    }

    #[test]
    fn ps_output_is_parsed() {
        let table = parse_ps(
            "  1   0   1   0 /sbin/launchd\n 42  1  42  43 -zsh\nbad line\n\
             43 42  43  43 claude --resume\n 44 1 44 -1 daemon\n",
        );
        assert_eq!(table.len(), 4);
        assert_eq!(table[2].args, "claude --resume");
        assert_eq!((table[2].pgid, table[2].tpgid), (43, 43));
        assert_eq!(table[3].tpgid, 0, "no terminal");
    }

    #[test]
    fn arguments_a_program_overwrote_are_cut_at_the_damage() {
        assert_eq!(
            clean_argv(["yes", "hello\nCOLORTERM=truecolor"].into_iter()),
            ["yes", "hello"]
        );
        assert_eq!(clean_argv(["a b", ""].into_iter()), ["a b", ""]);
    }

    #[test]
    fn the_foreground_job_is_the_shells_children_in_the_foreground_group() {
        let table = parse_ps(
            "10 1 10 10 -zsh\n11 10 11 10 sleep 100\n\
             20 1 20 21 -zsh\n21 20 21 21 tail -f log\n22 20 21 21 grep x\n\
             23 21 21 21 helper\n24 20 24 21 server\n",
        );
        assert!(
            foreground_job(&table, 10).is_empty(),
            "at the prompt; a background job doesn't count"
        );
        let job: Vec<&str> = foreground_job(&table, 20)
            .iter()
            .map(|p| p.args.as_str())
            .collect();
        assert_eq!(
            job,
            ["tail -f log", "grep x"],
            "a pipeline, not its helpers"
        );
        assert!(foreground_job(&table, 99).is_empty());
    }

    #[test]
    fn the_nearest_agent_under_the_shell_is_found() {
        let table = parse_ps(
            "10 1 10 0 -zsh\n11 10 11 0 node /x/bin/codex.js\n\
             12 11 11 0 /x/codex-aarch64-apple-darwin\n13 12 11 0 claude\n\
             20 1 20 0 -zsh\n21 20 21 0 vim\n30 1 30 0 claude\n",
        );
        assert_eq!(agent_under(&table, 10), Some(Agent::Codex));
        assert_eq!(
            agent_under(&table, 20),
            None,
            "a claude elsewhere doesn't count"
        );
        assert_eq!(
            agent_under(&table, 30),
            Some(Agent::Claude),
            "the root itself"
        );
        assert_eq!(agent_under(&table, 99), None);
        // A cycle doesn't hang.
        let cycle = parse_ps("1 2 1 0 sh\n2 1 2 0 sh\n");
        assert_eq!(agent_under(&cycle, 1), None);
    }
}
