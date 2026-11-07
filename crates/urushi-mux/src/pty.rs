//! The PTY boundary: spawning a process attached to a pseudo-terminal.
//!
//! [`NativePty`] uses `portable-pty`: Unix PTYs on macOS and Linux, `ConPTY` on Windows.
//! Tests and other backends implement [`Pty`] themselves.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::MuxError;
use crate::screen::TermSize;

/// What to run in a new pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnSpec {
    /// Program to run. `None` runs the user's login shell.
    pub program: Option<String>,
    /// Arguments (ignored when `program` is `None`).
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables, on top of the inherited environment.
    pub env: Vec<(String, String)>,
    /// Initial size.
    pub size: Option<TermSize>,
}

/// Control over a spawned process and its terminal.
pub trait PtyControl: Send {
    /// Tells the terminal (and so the process) its new size.
    fn resize(&self, size: TermSize) -> io::Result<()>;
    /// Terminates the process.
    fn kill(&mut self) -> io::Result<()>;
    /// The exit code, if the process has exited.
    fn try_wait(&mut self) -> io::Result<Option<i32>>;
    /// OS process id, if known.
    fn pid(&self) -> Option<u32>;
}

/// A running process attached to a PTY.
pub struct PtyProcess {
    /// Output from the process. Reads block.
    pub reader: Box<dyn Read + Send>,
    /// Input to the process. Writes may block.
    pub writer: Box<dyn Write + Send>,
    /// Size, kill and exit status.
    pub control: Box<dyn PtyControl>,
}

/// Something that can spawn processes on a PTY.
pub trait Pty: Send + Sync {
    /// Spawns `spec`.
    fn spawn(&self, spec: &SpawnSpec) -> Result<PtyProcess, MuxError>;
}

/// The operating system's PTY (via `portable-pty`).
#[derive(Clone, Copy, Debug, Default)]
pub struct NativePty;

impl Pty for NativePty {
    fn spawn(&self, spec: &SpawnSpec) -> Result<PtyProcess, MuxError> {
        let size = spec.size.unwrap_or(TermSize::DEFAULT);
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|e| MuxError::Spawn(e.to_string()))?;

        let mut command = match &spec.program {
            Some(program) => {
                let mut c = CommandBuilder::new(program);
                c.args(&spec.args);
                c
            }
            None => CommandBuilder::new_default_prog(),
        };
        if let Some(cwd) = &spec.cwd {
            command.cwd(cwd);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Urushi");
        for (key, value) in &spec.env {
            command.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| MuxError::Spawn(e.to_string()))?;
        // Release our copy of the slave side, so the reader sees EOF when the child exits.
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| MuxError::Spawn(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| MuxError::Spawn(e.to_string()))?;
        Ok(PtyProcess {
            reader,
            writer,
            control: Box::new(NativeControl {
                master: pair.master,
                child,
            }),
        })
    }
}

struct NativeControl {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyControl for NativeControl {
    fn resize(&self, size: TermSize) -> io::Result<()> {
        self.master.resize(pty_size(size)).map_err(io::Error::other)
    }

    fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| i32::try_from(status.exit_code()).unwrap_or(i32::MAX)))
    }

    fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }
}

fn pty_size(size: TermSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}
