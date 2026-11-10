//! Running a helper program (`curl`, `security`, `ps`) with a time limit.
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
        Err(format!("{program} failed ({status})"))
    }
}

fn run_status(
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, Vec<u8>), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{program}: no output"))?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = (&mut stdout).take(MAX_OUTPUT).read_to_end(&mut out);
        out
    });
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
    let out = reader
        .join()
        .map_err(|_| format!("{program}: reading its output failed"))?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    Ok((status, out))
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
    fn a_large_input_doesnt_stall_a_program_that_answers_while_reading() {
        // Larger than any pipe buffer: `cat` writes back before it has read it all.
        let input = vec![b'x'; 4 * 1024 * 1024];
        let out = run("cat", &[], Some(&input), Duration::from_secs(20)).unwrap();
        assert_eq!(out.len(), input.len());
    }
}
