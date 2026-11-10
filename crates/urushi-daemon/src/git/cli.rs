//! [`Git`] implemented by running the `git` binary.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use urushi_core::BranchName;

use super::{Git, GitError, StatusCounts, Upstream, WorktreeInfo};

/// Local operations should take milliseconds. This only catches hangs.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(60);
/// Network operations (pushes to delete remote branches).
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
/// Fetching a new thread's base. Thread creation waits for it, and falls back to the
/// local commit when it fails, so it is kept short.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(5);

/// Runs `git` from `PATH` (or an explicit binary).
#[derive(Clone, Debug)]
pub struct CliGit {
    program: PathBuf,
}

impl Default for CliGit {
    fn default() -> Self {
        Self {
            program: PathBuf::from("git"),
        }
    }
}

impl CliGit {
    /// Uses `program` instead of `git` from `PATH`.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Result<String, GitError> {
        self.run_with_timeout(dir, args, LOCAL_TIMEOUT)
    }

    fn run_with_timeout(
        &self,
        dir: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<String, GitError> {
        let command_line = args.join(" ");
        let mut child = Command::new(&self.program)
            .arg("-C")
            .arg(dir)
            .args(args)
            // Stable, parseable output, and never block waiting for a password prompt.
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    GitError::NotInstalled
                } else {
                    GitError::Failed {
                        command: command_line.clone(),
                        message: e.to_string(),
                    }
                }
            })?;

        // Drain the pipes on threads, so a large output can't fill a pipe and deadlock.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(GitError::TimedOut(command_line));
                }
                Ok(None) => std::thread::sleep(POLL),
                Err(e) => {
                    return Err(GitError::Failed {
                        command: command_line,
                        message: e.to_string(),
                    });
                }
            }
        };
        let out = stdout.join().unwrap_or_default();
        let err = stderr.join().unwrap_or_default();
        if status.success() {
            Ok(out)
        } else {
            let message = err.trim().to_owned();
            if message.contains("not a git repository") {
                return Err(GitError::NotARepository(dir.to_path_buf()));
            }
            Err(GitError::Failed {
                command: command_line,
                message,
            })
        }
    }
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        text
    })
}

fn lines(output: &str) -> impl Iterator<Item = &str> {
    output.lines().map(str::trim_end).filter(|l| !l.is_empty())
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Git for CliGit {
    fn repo_root(&self, path: &Path) -> Result<PathBuf, GitError> {
        let out = self.run(path, &["rev-parse", "--show-toplevel"])?;
        Ok(PathBuf::from(out.trim()))
    }

    fn current_branch(&self, repo: &Path) -> Result<Option<String>, GitError> {
        match self.run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
            Ok(out) => Ok(Some(out.trim().to_owned()).filter(|b| !b.is_empty())),
            Err(GitError::Failed { .. }) => Ok(None), // detached HEAD
            Err(e) => Err(e),
        }
    }

    fn resolve_commit(&self, repo: &Path, rev: &str) -> Result<String, GitError> {
        let spec = format!("{rev}^{{commit}}");
        let out = self.run(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &spec,
            ],
        )?;
        Ok(out.trim().to_owned())
    }

    fn local_branches(&self, repo: &Path) -> Result<Vec<String>, GitError> {
        let out = self.run(
            repo,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )?;
        Ok(lines(&out).map(str::to_owned).collect())
    }

    fn worktree_add(
        &self,
        repo: &Path,
        path: &Path,
        branch: &BranchName,
        base_commit: &str,
    ) -> Result<(), GitError> {
        self.run(
            repo,
            &[
                "worktree",
                "add",
                "-b",
                branch.as_str(),
                &path_str(path),
                base_commit,
            ],
        )
        .map(drop)
    }

    fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), GitError> {
        // Two -f: also remove locked worktrees.
        self.run(repo, &["worktree", "remove", "-f", "-f", &path_str(path)])
            .map(drop)
    }

    fn worktree_prune(&self, repo: &Path) -> Result<(), GitError> {
        self.run(repo, &["worktree", "prune"]).map(drop)
    }

    fn worktrees(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, GitError> {
        let out = self.run(repo, &["worktree", "list", "--porcelain"])?;
        let mut result = Vec::new();
        let mut current: Option<WorktreeInfo> = None;
        for line in out.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                result.extend(current.take());
                current = Some(WorktreeInfo {
                    path: PathBuf::from(path),
                    branch: None,
                });
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/")
                && let Some(wt) = current.as_mut()
            {
                wt.branch = Some(branch.to_owned());
            }
        }
        result.extend(current);
        Ok(result)
    }

    fn branches_checked_out_in(&self, worktree: &Path) -> Result<Vec<String>, GitError> {
        let mut branches = Vec::new();
        // HEAD's reflog is per worktree, so it only lists this worktree's checkouts.
        if let Ok(out) = self.run(worktree, &["log", "-g", "--format=%gs", "HEAD", "--"]) {
            for line in lines(&out) {
                if let Some(rest) = line.strip_prefix("checkout: moving from ")
                    && let Some((_, to)) = rest.rsplit_once(" to ")
                {
                    branches.push(to.to_owned());
                }
            }
        }
        if let Some(current) = self.current_branch(worktree)? {
            branches.push(current);
        }
        Ok(branches)
    }

    fn status_counts(&self, worktree: &Path) -> Result<StatusCounts, GitError> {
        let out = self.run(
            worktree,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?;
        let mut counts = StatusCounts::default();
        for line in lines(&out) {
            if line.starts_with("??") {
                counts.untracked += 1;
            } else {
                counts.modified += 1;
            }
        }
        Ok(counts)
    }

    fn unpushed_commits(
        &self,
        repo: &Path,
        branches: &[BranchName],
        base_commit: &str,
    ) -> Result<u32, GitError> {
        if branches.is_empty() {
            return Ok(0);
        }
        let mut args = vec!["rev-list", "--count"];
        args.extend(branches.iter().map(BranchName::as_str));
        args.extend(["--not", base_commit, "--remotes"]);
        let out = self.run(repo, &args)?;
        Ok(out.trim().parse().unwrap_or(0))
    }

    fn remote_branches(&self, repo: &Path) -> Result<Vec<(String, String)>, GitError> {
        let out = self.run(
            repo,
            &["for-each-ref", "--format=%(refname)", "refs/remotes"],
        )?;
        Ok(lines(&out)
            .filter_map(|r| r.strip_prefix("refs/remotes/"))
            .filter_map(|r| r.split_once('/'))
            .filter(|(_, branch)| *branch != "HEAD")
            .map(|(remote, branch)| (remote.to_owned(), branch.to_owned()))
            .collect())
    }

    fn remote_default_branches(&self, repo: &Path) -> Result<Vec<String>, GitError> {
        let out = self.run(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(symref)",
                "refs/remotes",
            ],
        )?;
        Ok(lines(&out)
            .filter_map(|l| l.split_once('\0'))
            .filter(|(name, target)| name.ends_with("/HEAD") && !target.is_empty())
            .filter_map(|(_, target)| target.strip_prefix("refs/remotes/"))
            .filter_map(|t| t.split_once('/').map(|(_, branch)| branch.to_owned()))
            .collect())
    }

    fn upstreams(&self, repo: &Path) -> Result<Vec<Upstream>, GitError> {
        let out = self.run(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname:short)%00%(upstream:remotename)%00%(upstream:remoteref)",
                "refs/heads",
            ],
        )?;
        Ok(lines(&out)
            .filter_map(|l| {
                let mut parts = l.split('\0');
                let local = parts.next()?;
                let remote = parts.next()?;
                let remote_ref = parts.next()?.strip_prefix("refs/heads/")?;
                (!remote.is_empty()).then(|| Upstream {
                    local: local.to_owned(),
                    remote: remote.to_owned(),
                    remote_branch: remote_ref.to_owned(),
                })
            })
            .collect())
    }

    fn delete_branch(&self, repo: &Path, branch: &BranchName) -> Result<(), GitError> {
        self.run(repo, &["branch", "-D", "--", branch.as_str()])
            .map(drop)
    }

    fn delete_remote_branch(
        &self,
        repo: &Path,
        remote: &str,
        branch: &str,
    ) -> Result<(), GitError> {
        let refspec = format!(":refs/heads/{branch}");
        self.run_with_timeout(
            repo,
            &["push", "--no-verify", remote, &refspec],
            NETWORK_TIMEOUT,
        )
        .map(drop)
    }

    fn fetch_branch(&self, repo: &Path, remote: &str, branch: &str) -> Result<(), GitError> {
        let refspec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
        self.run_with_timeout(
            repo,
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                "--end-of-options",
                remote,
                &refspec,
            ],
            FETCH_TIMEOUT,
        )
        .map(drop)
    }

    fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool, GitError> {
        // Commits reachable from `ancestor` but not from `descendant`: none iff it's an
        // ancestor. Unlike `merge-base --is-ancestor`, failures and "no" don't share an
        // exit code.
        let range = format!("{descendant}..{ancestor}");
        let out = self.run(repo, &["rev-list", "--count", "--end-of-options", &range])?;
        Ok(out.trim() == "0")
    }

    fn fast_forward_branch(
        &self,
        repo: &Path,
        branch: &BranchName,
        new: &str,
        old: &str,
    ) -> Result<(), GitError> {
        let reference = format!("refs/heads/{}", branch.as_str());
        self.run(
            repo,
            &[
                "update-ref",
                "-m",
                "urushi: update from remote",
                &reference,
                new,
                old,
            ],
        )
        .map(drop)
    }
}

#[cfg(test)]
mod tests;
