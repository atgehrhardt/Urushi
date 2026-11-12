//! One client connection: handshake, then a read loop that dispatches requests and
//! notifications. A single writer task serializes outgoing frames.
//!
//! Requests run concurrently (each in its own task, answered by id). Notifications
//! (keystrokes, acks, scrolling) are handled in order, inline, because their order matters.
//!
//! A connection works in both directions or not at all (ADR 0052): when the writer
//! can't write, the whole connection closes, so the client sees it end and reconnects
//! rather than waiting on answers that will never come.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use urushi_core::PaneId;
use urushi_proto::codec::{FrameError, read_envelope, write_envelope};
use urushi_proto::v1::{self, Envelope, envelope::Kind, notification, request};
use urushi_proto::{PROTOCOL_MAJOR, PROTOCOL_MINOR, convert, envelope};

use super::{attach, handlers, screen};
use crate::service::{Service, lock};

/// Frames queued for one client. Screen updates are flow-controlled, so this only needs
/// to absorb bursts of responses.
const OUTBOUND_QUEUE: usize = 256;

/// How long a new connection has to say `Hello`. A peer that connects and stays silent
/// would otherwise hold its socket (a file descriptor) for as long as the daemon runs.
pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The shortest gap between two `WorkspaceChanged` sent to one client. Changes in
/// between (agents starting and finishing, attention) coalesce into the next one, so
/// many busy agents can't make the daemon rebuild and resend the workspace hundreds of
/// times a second.
pub(crate) const WORKSPACE_EVENT_INTERVAL: Duration = Duration::from_millis(50);

/// Who is on the other end.
#[derive(Clone, Debug)]
pub(crate) struct Peer {
    /// For logging.
    pub(crate) description: String,
    /// Connected through the local socket or pipe (the same OS user on this machine),
    /// not over the tailnet or the LAN. Only local peers may stop the daemon or change
    /// LAN access.
    pub(crate) local: bool,
    /// Ends the connection when it changes or its sender is dropped: a LAN device that
    /// was revoked, or LAN access turned off. `None` for connections that only end
    /// when the client or the daemon goes.
    pub(crate) close: Option<tokio::sync::watch::Receiver<bool>>,
}

/// State shared by a connection's tasks.
pub(crate) struct Connection {
    pub(crate) service: Arc<Service>,
    pub(crate) out: mpsc::Sender<Envelope>,
    pub(crate) attachments: Mutex<HashMap<PaneId, attach::Attachment>>,
    pub(crate) subscription: Mutex<Option<JoinHandle<()>>>,
    /// The Jira board this connection shows (`WatchBoard`), read every few seconds.
    pub(crate) watching: Mutex<Option<crate::service::BoardWatch>>,
    /// See [`Peer::local`].
    pub(crate) local: bool,
}

impl Connection {
    /// Streams `WorkspaceChanged` to this client from now on (idempotent).
    pub(crate) fn subscribe_workspace(self: &Arc<Self>) {
        let mut slot = lock(&self.subscription);
        if slot.is_some() {
            return;
        }
        let mut changes = self.service.subscribe();
        changes.mark_unchanged();
        let service = self.service.clone();
        let out = self.out.clone();
        *slot = Some(tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                let event = envelope::event(v1::event::Body::WorkspaceChanged(service.workspace()));
                if out.send(event).await.is_err() {
                    break;
                }
                tokio::time::sleep(WORKSPACE_EVENT_INTERVAL).await;
            }
        }));
    }

    fn close(&self) {
        if let Some(task) = lock(&self.subscription).take() {
            task.abort();
        }
        for (_, attachment) in lock(&self.attachments).drain() {
            attachment.stop();
        }
        lock(&self.watching).take();
    }
}

/// [`serve`] as a boxed future. Request handling can reconfigure the tailnet listener,
/// which in turn serves connections, so that path needs a concrete type to break the
/// recursive `impl Future` cycle.
pub(crate) fn serve_boxed<S>(
    service: Arc<Service>,
    stream: S,
    peer: Peer,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    Box::pin(serve(service, stream, peer))
}

/// Serves one connection until the client disconnects, the daemon stops, the
/// connection's access ends, or the daemon can no longer write to it.
pub(crate) async fn serve<S>(service: Arc<Service>, stream: S, peer: Peer)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, writer) = tokio::io::split(stream);
    let (out, outbound) = mpsc::channel::<Envelope>(OUTBOUND_QUEUE);
    let mut writer_task = tokio::spawn(write_frames(writer, outbound, peer.description.clone()));

    let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake(&mut reader, &out)).await;
    if let Err(reason) = handshake.unwrap_or_else(|_| Err("no hello in time".into())) {
        tracing::info!(peer = %peer.description, %reason, "rejected connection");
        drop(out);
        let _ = writer_task.await;
        return;
    }
    tracing::debug!(peer = %peer.description, "client connected");

    let connection = Arc::new(Connection {
        service,
        out,
        attachments: Mutex::new(HashMap::new()),
        subscription: Mutex::new(None),
        watching: Mutex::new(None),
        local: peer.local,
    });
    let mut close = peer.close.clone();
    let closed = async move {
        match close.as_mut() {
            Some(close) => {
                let _ = close.changed().await;
            }
            None => std::future::pending().await,
        }
    };
    tokio::pin!(closed);
    loop {
        let read = tokio::select! {
            read = read_envelope(&mut reader) => read,
            () = &mut closed => {
                tracing::info!(peer = %peer.description, "closing a connection whose access ended");
                writer_task.abort();
                break;
            }
            // A stopped daemon hangs up, so clients see it go and a pipe or socket of
            // the old daemon never lingers (on Windows it would keep the next daemon
            // from creating its pipe).
            () = connection.service.stopped() => {
                writer_task.abort();
                break;
            }
            // The writer gave up: nothing more can reach the client, so hang up and
            // let it reconnect.
            _ = &mut writer_task => break,
        };
        match read {
            Ok(Some(frame)) => dispatch(&connection, frame).await,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(peer = %peer.description, error = %e, "connection error");
                break;
            }
        }
    }
    writer_task.abort();
    connection.close();
    tracing::debug!(peer = %peer.description, "client disconnected");
}

/// Writes queued frames until the queue closes or a write fails. A frame too large to
/// send is dropped and logged instead of ending the connection; if it answered a
/// request, the request gets an error in its place, so the client isn't left waiting.
async fn write_frames<W>(mut writer: W, mut outbound: mpsc::Receiver<Envelope>, peer: String)
where
    W: AsyncWrite + Unpin,
{
    while let Some(frame) = outbound.recv().await {
        match write_envelope(&mut writer, &frame).await {
            Ok(()) => {}
            Err(FrameError::TooLarge(len)) => {
                tracing::error!(%peer, len, kind = frame_kind(&frame), "dropped a frame too large to send");
                if let Some(Kind::Response(response)) = &frame.kind {
                    let error = envelope::error(
                        response.id,
                        v1::ErrorCode::Internal,
                        format!("the answer ({len} bytes) is too large to send"),
                    );
                    if write_envelope(&mut writer, &error).await.is_err() {
                        break;
                    }
                }
            }
            Err(error) => {
                tracing::info!(%peer, %error, "could not write to a client; closing the connection");
                break;
            }
        }
    }
}

/// A frame's kind, for logs.
fn frame_kind(frame: &Envelope) -> &'static str {
    match &frame.kind {
        Some(Kind::Response(_)) => "response",
        Some(Kind::Event(v1::Event {
            body: Some(v1::event::Body::WorkspaceChanged(_)),
        })) => "WorkspaceChanged",
        Some(Kind::Event(_)) => "event",
        _ => "other",
    }
}

async fn handshake<R>(reader: &mut R, out: &mpsc::Sender<Envelope>) -> Result<(), String>
where
    R: AsyncRead + Unpin,
{
    let first = read_envelope(reader)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("closed before hello")?;
    let Some(Kind::Request(v1::Request {
        id,
        body: Some(request::Body::Hello(hello)),
    })) = first.kind
    else {
        return Err("first message was not Hello".into());
    };
    if hello.protocol_major != PROTOCOL_MAJOR {
        let message = format!(
            "protocol {}.x is not supported (daemon speaks {PROTOCOL_MAJOR}.x)",
            hello.protocol_major
        );
        let _ = out
            .send(envelope::error(id, v1::ErrorCode::Unsupported, &message))
            .await;
        return Err(message);
    }
    let welcome = v1::Welcome {
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
        host_name: super::host_name(),
    };
    out.send(envelope::response(
        id,
        v1::response::Outcome::Welcome(welcome),
    ))
    .await
    .map_err(|e| e.to_string())
}

async fn dispatch(connection: &Arc<Connection>, frame: Envelope) {
    match frame.kind {
        Some(Kind::Request(v1::Request { id, body })) => {
            let connection = connection.clone();
            tokio::spawn(async move {
                let outcome = match body {
                    Some(body) => handlers::handle(&connection, body).await,
                    None => handlers::invalid("empty request"),
                };
                let _ = connection.out.send(envelope::response(id, outcome)).await;
            });
        }
        Some(Kind::Notification(v1::Notification { body: Some(body) })) => {
            notify(connection, body).await;
        }
        _ => tracing::debug!("ignoring unexpected frame from client"),
    }
}

async fn notify(connection: &Arc<Connection>, body: notification::Body) {
    match body {
        notification::Body::Input(input) => {
            let Some((pane, input)) = screen::input_from_proto(input) else {
                return;
            };
            // Typing or clicking in a pane means the user has seen it, and takes it
            // over from a task message waiting to be drafted there.
            if is_user_action(&input) {
                connection.service.user_acted(pane);
            }
            // Claude runs no hook when it is interrupted (ADR 0039).
            if crate::service::is_interrupt(&input) {
                connection.service.user_interrupted(pane);
            }
            if let Ok(session) = connection.service.session(pane) {
                let _ = session.input(input).await;
            }
        }
        notification::Body::AckScreen(ack) => {
            if let Ok(pane) = convert::parse_id::<PaneId>(&ack.pane_id, "pane_id")
                && let Some(a) = lock(&connection.attachments).get(&pane)
            {
                a.ack(ack.seq);
            }
        }
        notification::Body::ScrollPane(scroll) => {
            let Ok(pane) = convert::parse_id::<PaneId>(&scroll.pane_id, "pane_id") else {
                return;
            };
            if let Ok(session) = connection.service.session(pane) {
                let request = if scroll.to_bottom {
                    urushi_mux::Scroll::Bottom
                } else {
                    urushi_mux::Scroll::Lines(scroll.lines)
                };
                let _ = session.scroll(request).await;
            }
        }
    }
}

/// Whether `input` is the user acting on the pane (not the pointer passing over it or
/// the wheel scrolling it).
fn is_user_action(input: &urushi_mux::Input) -> bool {
    use urushi_mux::{Input, MouseAction};
    match input {
        Input::Key(_) | Input::Text(_) | Input::Paste(_) | Input::Bytes(_) => true,
        Input::Mouse(m) => m.action == MouseAction::Press,
        Input::Wheel(_) => false,
    }
}

#[cfg(test)]
mod tests;
