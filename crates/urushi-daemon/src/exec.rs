//! Running a helper program (`curl`, `security`, `ps`, `tailscale`) with a time limit.
//! Blocking: call it off the async runtime.
//!
//! Input and output each move on their own thread, so a program that answers while it
//! still reads a large input (curl sending a request body) can't stall the pipe, and
//! the time limit always applies.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Longest output read from a helper; more is a misbehaving program.
const MAX_OUTPUT: u64 = 8 * 1024 * 1024;

/// Runs `program args…`, writes `input` to its stdin, and returns its stdout if it
/// exits successfully within `timeout`.
pub(crate) fn run(
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let (status, out) = run_status(program, args, input, timeout)?;
    if status.success() {
        Ok(out)
    } else {
        Err(failure(program, status))
    }
}

/// Why `program` failed, the same on every OS: `… failed (exit status: N)` (Windows would
/// otherwise say "exit code"), so callers can tell failures apart by their code.
fn failure(program: &str, status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("{program} failed (exit status: {code})"),
        None => format!("{program} failed ({status})"),
    }
}

fn run_status(
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, Vec<u8>), String> {
    let mut command = Command::new(program);
    command.args(args);
    let finished = run_command(command, program, input, timeout)?;
    Ok((finished.status, finished.stdout))
}

/// A helper program that ran to its end.
pub(crate) struct Finished {
    /// How it exited.
    pub(crate) status: std::process::ExitStatus,
    /// What it wrote to stdout (at most 8 MiB).
    pub(crate) stdout: Vec<u8>,
    /// What it wrote to stderr (at most 8 MiB), e.g. why it failed.
    pub(crate) stderr: Vec<u8>,
}

/// Runs `command` (its program and arguments, and any environment, set by the caller),
/// writes `input` to its stdin, and collects its output, if it exits within `timeout`.
/// `program` names it in errors. Otherwise it is killed.
pub(crate) fn run_command(
    mut command: Command,
    program: &str,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<Finished, String> {
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let writer = match (input, child.stdin.take()) {
        (Some(input), Some(mut stdin)) => {
            let input = input.to_vec();
            // Dropping stdin when done closes it, so the program sees end of input. A
            // program that exits without reading it all is not an error here.
            Some(std::thread::spawn(move || {
                let _ = stdin.write_all(&input);
            }))
        }
        _ => None,
    };
    let stdout = read_all(
        child
            .stdout
            .take()
            .ok_or_else(|| format!("{program}: no output"))?,
    );
    let stderr = read_all(
        child
            .stderr
            .take()
            .ok_or_else(|| format!("{program}: no error output"))?,
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} timed out"));
            }
            Err(e) => return Err(format!("{program}: {e}")),
        }
    };
    let stdout = stdout
        .join()
        .map_err(|_| format!("{program}: reading its output failed"))?;
    let stderr = stderr.join().unwrap_or_default();
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    Ok(Finished {
        status,
        stdout,
        stderr,
    })
}

/// Reads `pipe` to its end (at most [`MAX_OUTPUT`]) on a thread of its own.
fn read_all(pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = pipe.take(MAX_OUTPUT).read_to_end(&mut out);
        out
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn input_output_failure_and_timeout() {
        let out = run("cat", &[], Some(b"hello"), Duration::from_secs(5)).unwrap();
        assert_eq!(out, b"hello");
        assert!(run("false", &[], None, Duration::from_secs(5)).is_err());
        let started = Instant::now();
        let slow = run("sleep", &["5"], None, Duration::from_millis(100));
        assert!(slow.unwrap_err().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(run("/no/such/program", &[], None, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn a_command_gets_its_environment_and_its_errors_are_kept() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "echo \"$GREETING\"; echo oops >&2; exit 3"])
            .env("GREETING", "hi");
        let out = run_command(command, "sh", None, Duration::from_secs(5)).unwrap();
        assert_eq!(out.stdout, b"hi\n");
        assert_eq!(out.stderr, b"oops\n");
        assert_eq!(out.status.code(), Some(3));
    }

    #[test]
    fn a_large_input_doesnt_stall_a_program_that_answers_while_reading() {
        // Larger than any pipe buffer: `cat` writes back before it has read it all.
        let input = vec![b'x'; 4 * 1024 * 1024];
        let out = run("cat", &[], Some(&input), Duration::from_secs(20)).unwrap();
        assert_eq!(out.len(), input.len());
    }
}

#[cfg(test)]
mod exit_status_tests {
    use super::*;

    /// Callers (e.g. `http::friendly`) tell failures apart by their exit code, so it
    /// must read the same on every OS.
    #[test]
    fn failures_name_their_exit_status() {
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/c", "exit 7"])
        } else {
            ("sh", &["-c", "exit 7"])
        };
        let error = run(program, args, None, Duration::from_secs(10)).unwrap_err();
        assert_eq!(error, format!("{program} failed (exit status: 7)"));
    }
}
