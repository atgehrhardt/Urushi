//! Streaming a pane's screen to one client, with flow control.
//!
//! After `AttachPane` returns the first full frame (seq 1), a task per attachment waits
//! for the client's `AckScreen` before sending the next update. It then sends only what
//! changed since the last frame it sent. A slow client gets fewer, larger updates instead
//! of a growing queue. Updates are also limited to about 120 per second.
//!
//! An ack that never comes (a client dropped it under load, ADR 0048) holds the stream up
//! for at most [`ACK_TIMEOUT`]: then the update counts as acknowledged. The next frame is
//! still a correct diff, from the last frame *sent*, so nothing on screen is lost.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use urushi_core::PaneId;
use urushi_mux::{SessionHandle, TermSize};
use urushi_proto::envelope;
use urushi_proto::v1::{self, Envelope, event};

use super::connection::Connection;
use super::screen;
use crate::service::{ServiceResult, lock};

const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(8);

/// Longest the stream waits for an `AckScreen` before sending the next update anyway.
/// A client that is merely slow still gets at most one update per this interval.
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// A live attachment of one pane to one connection.
pub(crate) struct Attachment {
    acks: watch::Sender<u64>,
    task: JoinHandle<()>,
}

impl Attachment {
    /// Records that the client has applied update `seq`.
    pub(crate) fn ack(&self, seq: u64) {
        self.acks.send_if_modified(|acked| {
            let newer = seq > *acked;
            if newer {
                *acked = seq;
            }
            newer
        });
    }

    /// Stops streaming.
    pub(crate) fn stop(self) {
        self.task.abort();
    }
}

impl Connection {
    /// Attaches `pane` at `size` and returns the first full frame (seq 1). Attaching a
    /// pane that is already attached on this connection restarts its stream.
    pub(crate) async fn attach(
        self: &Arc<Self>,
        pane: PaneId,
        size: TermSize,
    ) -> ServiceResult<v1::ScreenUpdate> {
        let session = self.service.session(pane)?;
        session.resize(size).await?;
        let frame = session.frame_since(0).await?;
        let first = screen::to_proto(pane, 1, &frame);
        let (acks, ack_rx) = watch::channel(0);
        let task = tokio::spawn(stream(
            session,
            pane,
            self.out.clone(),
            ack_rx,
            frame.version,
        ));
        if let Some(previous) = lock(&self.attachments).insert(pane, Attachment { acks, task }) {
            previous.stop();
        }
        Ok(first)
    }

    /// Stops streaming `pane` to this connection.
    pub(crate) fn detach(&self, pane: PaneId) {
        if let Some(attachment) = lock(&self.attachments).remove(&pane) {
            attachment.stop();
        }
    }
}

async fn stream(
    session: SessionHandle,
    pane: PaneId,
    out: mpsc::Sender<Envelope>,
    acks: watch::Receiver<u64>,
    sent_version: u64,
) {
    let mut status = session.status();
    let ended = pump(&session, pane, &out, acks, &mut status, sent_version).await;
    if ended == Ended::SessionGone {
        // However the session ended (exit, or killed after exiting), the client is told once.
        let code = status.borrow().exit_code.unwrap_or(-1);
        let exited = envelope::event(event::Body::PaneExited(v1::PaneExited {
            pane_id: pane.to_string(),
            exit_code: code,
        }));
        let _ = out.send(exited).await;
    }
}

#[derive(PartialEq, Eq)]
enum Ended {
    /// The process exited or the session stopped.
    SessionGone,
    /// The client went away.
    ClientGone,
}

async fn pump(
    session: &SessionHandle,
    pane: PaneId,
    out: &mpsc::Sender<Envelope>,
    mut acks: watch::Receiver<u64>,
    status: &mut watch::Receiver<urushi_mux::SessionStatus>,
    mut sent_version: u64,
) -> Ended {
    let mut seq: u64 = 1;
    // The first frame went out in `AttachPane`'s answer just now.
    let mut sent_at = Instant::now();
    let mut bells_seen = status.borrow().bells;
    let pane_id = pane.to_string();
    loop {
        let current = *status.borrow_and_update();
        if current.bells > bells_seen {
            bells_seen = current.bells;
            let bell = envelope::event(event::Body::PaneBell(v1::PaneBell {
                pane_id: pane_id.clone(),
            }));
            if out.send(bell).await.is_err() {
                return Ended::ClientGone;
            }
        }
        let acked = *acks.borrow_and_update() >= seq || sent_at.elapsed() >= ACK_TIMEOUT;
        if acked && current.version > sent_version {
            let Ok(frame) = session.frame_since(sent_version).await else {
                return Ended::SessionGone;
            };
            seq += 1;
            sent_version = frame.version;
            let update = envelope::event(event::Body::ScreenUpdate(screen::to_proto(
                pane, seq, &frame,
            )));
            if out.send(update).await.is_err() {
                return Ended::ClientGone;
            }
            sent_at = Instant::now();
            tokio::time::sleep(MIN_FRAME_INTERVAL).await;
            continue;
        }
        if current.exit_code.is_some() && (current.version <= sent_version || !acked) {
            return Ended::SessionGone;
        }
        tokio::select! {
            changed = status.changed() => if changed.is_err() { return Ended::SessionGone },
            changed = acks.changed() => if changed.is_err() { return Ended::ClientGone },
            // Only while an update waits for its ack: then stop waiting.
            () = tokio::time::sleep_until(sent_at + ACK_TIMEOUT), if !acked => {}
        }
    }
}
