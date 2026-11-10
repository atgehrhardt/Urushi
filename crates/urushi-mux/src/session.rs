//! A pane's session: one process on a PTY plus its emulator, owned by an actor task.
//!
//! ```text
//!  reader thread ──bytes──► actor (owns Emulator) ──bytes──► writer thread ──► PTY
//!                              ▲  │
//!         SessionHandle ──cmd──┘  └── watch<SessionStatus> (version, bells, exit)
//! ```
//!
//! Blocking PTY reads and writes happen on dedicated OS threads, so a stalled child never
//! blocks the async runtime. The actor is the only owner of the emulator, so there are no
//! locks. Clients poll for changes with [`SessionHandle::frame_since`], passing the last
//! version they saw. That merges bursts of output into one update and needs no
//! per-client queue.

use std::io::{Read, Write};
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use urushi_core::AttentionKind;

use crate::MuxError;
use crate::emulator::{Damage, Emulator, EmulatorEvent, Scroll};
use crate::keys::{KeyEvent, encode_key, encode_paste};
use crate::mouse::{MouseEvent, WheelAction, WheelEvent, encode_mouse, wheel};
use crate::pty::{Pty, PtyControl, SpawnSpec};
use crate::replay::Snapshot;
use crate::screen::{ScreenFrame, TermSize};

/// Commands queued to the actor. Bounded, so a flood of input applies backpressure.
const COMMAND_QUEUE: usize = 256;
/// PTY output chunks buffered between the reader thread and the actor.
const OUTPUT_QUEUE: usize = 64;
/// Bytes read from the PTY at a time.
const READ_CHUNK: usize = 64 * 1024;
/// How long to wait for an exit code after the PTY closes.
const EXIT_POLL: Duration = Duration::from_millis(20);
const EXIT_POLL_ATTEMPTS: u32 = 100;

/// Input for a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// A key press, encoded according to the terminal's modes.
    Key(KeyEvent),
    /// Typed text, sent as is.
    Text(String),
    /// A clipboard paste (bracketed if the application asked for that).
    Paste(String),
    /// A mouse button or movement, reported if the application asked for the mouse.
    Mouse(MouseEvent),
    /// The wheel: reported to the application, turned into arrow keys, or scrolling the
    /// scrollback, depending on its modes (see [`crate::mouse::wheel`]).
    Wheel(WheelEvent),
    /// Raw bytes, for tests and scripting.
    Bytes(Vec<u8>),
}

/// Observable session state. Watch it with [`SessionHandle::status`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionStatus {
    /// Increases whenever screen content changes.
    pub version: u64,
    /// Total bells rung so far.
    pub bells: u64,
    /// Times the program asked for the user so far: bells, terminal notifications and
    /// Urushi's agent signals.
    pub alerts: u64,
    /// What the latest alert asked for (a bell is [`AttentionKind::Waiting`]).
    pub alert: Option<AttentionKind>,
    /// Times an agent's hook said it reads input so far (Urushi's `ready` signal). Not
    /// an alert.
    pub ready: u64,
    /// Exit code once the process has exited. `-1` if it couldn't be determined.
    pub exit_code: Option<i32>,
}

enum Command {
    Input(Input),
    Resize(TermSize),
    Scroll(Scroll),
    Frame {
        since: u64,
        reply: oneshot::Sender<ScreenFrame>,
    },
    Text {
        reply: oneshot::Sender<Vec<String>>,
    },
    Snapshot {
        max_lines: usize,
        reply: oneshot::Sender<Snapshot>,
    },
    Kill,
}

enum Output {
    Bytes(Vec<u8>),
    Closed,
}

/// A cheap, cloneable handle to a session actor. When the last handle is dropped, the
/// process is killed.
#[derive(Clone)]
pub struct SessionHandle {
    commands: mpsc::Sender<Command>,
    status: watch::Receiver<SessionStatus>,
    pid: Option<u32>,
}

impl SessionHandle {
    /// Spawns `spec` on `pty`, with `emulator` interpreting its output. Must be called
    /// inside a tokio runtime.
    pub fn spawn(
        pty: &dyn Pty,
        spec: &SpawnSpec,
        emulator: Box<dyn Emulator>,
    ) -> Result<Self, MuxError> {
        let process = pty.spawn(spec)?;
        let pid = process.control.pid();

        let (output_tx, output_rx) = mpsc::channel(OUTPUT_QUEUE);
        spawn_reader(process.reader, output_tx)?;
        let writer = spawn_writer(process.writer)?;

        let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE);
        let (status_tx, status_rx) = watch::channel(SessionStatus::default());
        let actor = Actor {
            emulator,
            control: process.control,
            writer,
            status: status_tx,
            title: String::new(),
            version: 0,
            line_versions: Vec::new(),
            reset_version: 0,
        };
        tokio::spawn(actor.run(command_rx, output_rx));
        Ok(Self {
            commands: command_tx,
            status: status_rx,
            pid,
        })
    }

    /// OS process id of the pane's process, if known.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// A receiver that is notified whenever the status changes.
    pub fn status(&self) -> watch::Receiver<SessionStatus> {
        self.status.clone()
    }

    /// Sends input. Scrolls the view back to live output.
    pub async fn input(&self, input: Input) -> Result<(), MuxError> {
        self.send(Command::Input(input)).await
    }

    /// Resizes the terminal.
    pub async fn resize(&self, size: TermSize) -> Result<(), MuxError> {
        self.send(Command::Resize(size)).await
    }

    /// Moves the scrollback view.
    pub async fn scroll(&self, scroll: Scroll) -> Result<(), MuxError> {
        self.send(Command::Scroll(scroll)).await
    }

    /// Screen changes since `since` (a version from an earlier frame). Pass `0` for a
    /// full frame.
    pub async fn frame_since(&self, since: u64) -> Result<ScreenFrame, MuxError> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Frame { since, reply }).await?;
        rx.await.map_err(|_| MuxError::Closed)
    }

    /// Plain text of each visible line, trailing blanks removed.
    pub async fn text(&self) -> Result<Vec<String>, MuxError> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Text { reply }).await?;
        rx.await.map_err(|_| MuxError::Closed)
    }

    /// The pane's last `max_lines` lines of output, to save for a restart (see
    /// [`crate::replay`]).
    pub async fn snapshot(&self, max_lines: usize) -> Result<Snapshot, MuxError> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Snapshot { max_lines, reply }).await?;
        rx.await.map_err(|_| MuxError::Closed)
    }

    /// Kills the process and stops the actor.
    pub async fn kill(&self) -> Result<(), MuxError> {
        self.send(Command::Kill).await
    }

    async fn send(&self, command: Command) -> Result<(), MuxError> {
        self.commands
            .send(command)
            .await
            .map_err(|_| MuxError::Closed)
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: mpsc::Sender<Output>,
) -> Result<(), MuxError> {
    std::thread::Builder::new()
        .name("urushi-pty-reader".into())
        .spawn(move || {
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.blocking_send(Output::Bytes(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = tx.blocking_send(Output::Closed);
        })
        .map(|_| ())
        .map_err(MuxError::Io)
}

fn spawn_writer(
    mut writer: Box<dyn Write + Send>,
) -> Result<std_mpsc::SyncSender<Vec<u8>>, MuxError> {
    let (tx, rx) = std_mpsc::sync_channel::<Vec<u8>>(COMMAND_QUEUE);
    std::thread::Builder::new()
        .name("urushi-pty-writer".into())
        .spawn(move || {
            for bytes in rx {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        })
        .map_err(MuxError::Io)?;
    Ok(tx)
}

struct Actor {
    emulator: Box<dyn Emulator>,
    control: Box<dyn PtyControl>,
    writer: std_mpsc::SyncSender<Vec<u8>>,
    status: watch::Sender<SessionStatus>,
    title: String,
    /// Current content version.
    version: u64,
    /// For each viewport line, the version at which it last changed.
    line_versions: Vec<u64>,
    /// Frames requested from before this version must be full (resize or full damage).
    reset_version: u64,
}

impl Actor {
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut output: mpsc::Receiver<Output>,
    ) {
        self.record_damage();
        let mut pty_open = true;
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Kill) | None => break,
                    Some(command) => self.handle(command),
                },
                chunk = output.recv(), if pty_open => match chunk {
                    Some(Output::Bytes(bytes)) => self.process_output(&bytes),
                    Some(Output::Closed) | None => {
                        pty_open = false;
                        let code = self.wait_for_exit().await;
                        self.status.send_modify(|s| s.exit_code = Some(code));
                    }
                },
            }
        }
        if pty_open {
            let _ = self.control.kill();
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Input(input) => {
                let modes = self.emulator.modes();
                let bytes = match input {
                    Input::Key(key) => encode_key(&key, modes),
                    Input::Text(text) => text.into_bytes(),
                    Input::Paste(text) => encode_paste(&text, modes),
                    Input::Bytes(bytes) => bytes,
                    Input::Mouse(event) => encode_mouse(&event, modes),
                    Input::Wheel(event) => match wheel(&event, modes) {
                        WheelAction::Send(bytes) => bytes,
                        WheelAction::Scrollback(lines) => {
                            self.emulator.scroll(Scroll::Lines(lines));
                            self.record_damage();
                            return;
                        }
                    },
                };
                if !bytes.is_empty() {
                    if self.emulator.display_offset() > 0 {
                        self.emulator.scroll(Scroll::Bottom);
                        self.record_damage();
                    }
                    self.write(bytes);
                }
            }
            Command::Resize(size) => {
                if size != self.emulator.size() {
                    self.emulator.resize(size);
                    if let Err(e) = self.control.resize(size) {
                        tracing::warn!(error = %e, "failed to resize pty");
                    }
                    self.record_damage();
                }
            }
            Command::Scroll(scroll) => {
                self.emulator.scroll(scroll);
                self.record_damage();
            }
            Command::Frame { since, reply } => {
                let _ = reply.send(self.frame_since(since));
            }
            Command::Text { reply } => {
                let rows = self.emulator.size().rows;
                let _ = reply.send(
                    (0..rows)
                        .map(|l| self.emulator.line(l).text().trim_end().to_owned())
                        .collect(),
                );
            }
            Command::Snapshot { max_lines, reply } => {
                let _ = reply.send(Snapshot {
                    size: self.emulator.size(),
                    alt_screen: self.emulator.modes().alt_screen,
                    lines: self.emulator.history(max_lines),
                });
            }
            Command::Kill => {}
        }
    }

    fn process_output(&mut self, bytes: &[u8]) {
        let mut bells = 0;
        let mut alerts = 0;
        let mut alert = None;
        let mut ready = 0;
        for event in self.emulator.advance(bytes) {
            match event {
                EmulatorEvent::Reply(reply) => self.write(reply),
                EmulatorEvent::Title(title) => self.title = title.unwrap_or_default(),
                EmulatorEvent::Bell => {
                    bells += 1;
                    alerts += 1;
                    alert = Some(AttentionKind::Waiting);
                }
                EmulatorEvent::Attention(kind) => {
                    alerts += 1;
                    alert = Some(kind);
                }
                EmulatorEvent::Ready => ready += 1,
            }
        }
        self.record_damage();
        if alerts > 0 || ready > 0 {
            self.status.send_modify(|s| {
                s.bells += bells;
                s.alerts += alerts;
                if alerts > 0 {
                    s.alert = alert;
                }
                s.ready += ready;
            });
        }
    }

    fn write(&self, bytes: Vec<u8>) {
        // A full queue means the child isn't reading its input. Dropping keystrokes is
        // better than blocking the actor, which would freeze every client.
        if self.writer.try_send(bytes).is_err() {
            tracing::warn!("pty input queue full; dropping input");
        }
    }

    /// Turns emulator damage into per-line versions and notifies watchers.
    fn record_damage(&mut self) {
        let rows = usize::from(self.emulator.size().rows);
        let damage = self.emulator.take_damage();
        if self.line_versions.len() != rows {
            self.line_versions.resize(rows, 0);
        }
        let next = self.version + 1;
        match damage {
            Damage::Full => {
                self.line_versions.iter_mut().for_each(|v| *v = next);
                self.reset_version = next;
            }
            Damage::Lines(lines) if lines.is_empty() => return,
            Damage::Lines(lines) => {
                for line in lines {
                    if let Some(v) = self.line_versions.get_mut(usize::from(line)) {
                        *v = next;
                    }
                }
            }
        }
        self.version = next;
        self.status.send_modify(|s| s.version = next);
    }

    fn frame_since(&self, since: u64) -> ScreenFrame {
        let full = since == 0 || since < self.reset_version;
        let lines = self
            .line_versions
            .iter()
            .enumerate()
            .filter(|(_, v)| full || **v > since)
            .filter_map(|(i, _)| u16::try_from(i).ok())
            .map(|i| (i, self.emulator.line(i)))
            .collect();
        ScreenFrame {
            version: self.version,
            full,
            size: self.emulator.size(),
            lines,
            cursor: self.emulator.cursor(),
            modes: self.emulator.modes(),
            title: self.title.clone(),
            display_offset: self.emulator.display_offset(),
            history_size: self.emulator.history_size(),
        }
    }

    async fn wait_for_exit(&mut self) -> i32 {
        for _ in 0..EXIT_POLL_ATTEMPTS {
            match self.control.try_wait() {
                Ok(Some(code)) => return code,
                Ok(None) => tokio::time::sleep(EXIT_POLL).await,
                Err(_) => break,
            }
        }
        -1
    }
}
