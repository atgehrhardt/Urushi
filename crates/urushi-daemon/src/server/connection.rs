//! One client connection: handshake, then a read loop that dispatches requests and
//! notifications. A single writer task serializes outgoing frames.
//!
//! Requests run concurrently (each in its own task, answered by id). Notifications
//! (keystrokes, acks, scrolling) are handled in order, inline, because their order matters.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use urushi_core::PaneId;
use urushi_proto::codec::{read_envelope, write_envelope};
use urushi_proto::v1::{self, Envelope, envelope::Kind, notification, request};
use urushi_proto::{PROTOCOL_MAJOR, PROTOCOL_MINOR, convert, envelope};

use super::{attach, handlers, screen};
use crate::service::{Service, lock};

/// Frames queued for one client. Screen updates are flow-controlled, so this only needs
/// to absorb bursts of responses.
const OUTBOUND_QUEUE: usize = 256;

/// Who is on the other end.
#[derive(Clone, Debug)]
pub(crate) struct Peer {
    /// For logging.
    pub(crate) description: String,
    /// Connected through the local socket or pipe (the same OS user on this machine),
    /// not over the tailnet. Only local peers may stop the daemon.
    pub(crate) local: bool,
}

/// State shared by a connection's tasks.
pub(crate) struct Connection {
    pub(crate) service: Arc<Service>,
    pub(crate) out: mpsc::Sender<Envelope>,
    pub(crate) attachments: Mutex<HashMap<PaneId, attach::Attachment>>,
    pub(crate) subscription: Mutex<Option<JoinHandle<()>>>,
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

/// Serves one connection until the client disconnects.
pub(crate) async fn serve<S>(service: Arc<Service>, stream: S, peer: Peer)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (out, mut outbound) = mpsc::channel::<Envelope>(OUTBOUND_QUEUE);
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = outbound.recv().await {
            if write_envelope(&mut writer, &frame).await.is_err() {
                break;
            }
        }
    });

    if let Err(reason) = handshake(&mut reader, &out).await {
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
        local: peer.local,
    });
    loop {
        match read_envelope(&mut reader).await {
            Ok(Some(frame)) => dispatch(&connection, frame).await,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(peer = %peer.description, error = %e, "connection error");
                break;
            }
        }
    }
    connection.close();
    tracing::debug!(peer = %peer.description, "client disconnected");
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
            // Typing or clicking in a pane means the user has seen it.
            if is_user_action(&input) {
                connection.service.clear_attention([pane]);
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
