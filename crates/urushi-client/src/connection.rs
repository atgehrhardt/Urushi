//! A connection to a daemon: handshake, request/response matching, and the event stream.
//!
//! A connection either works both ways or ends (ADR 0052). It ends when the daemon
//! closes it, when a write fails, when every [`Client`] clone is dropped, or when nothing
//! at all has arrived from the daemon for [`LIVENESS_TIMEOUT`] although the client kept
//! asking ([`Ping`](request::Body::Ping) every [`PING_INTERVAL`] of quiet). Ending closes the
//! socket, fails requests in flight with [`ClientError::Disconnected`], and ends the
//! [`Events`] stream, which is how applications learn to reconnect.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;
use urushi_proto::codec::{read_envelope, write_envelope};
use urushi_proto::endpoint::Endpoint;
use urushi_proto::envelope;
use urushi_proto::v1::{self, envelope::Kind, event, notification, request, response::Outcome};

use crate::ClientError;
use crate::platform;

/// Events buffered for the application. Screen updates are flow-controlled by acks, so
/// this only needs to absorb bursts.
const EVENT_QUEUE: usize = 1024;
/// Frames waiting to be written. Absorbs bursts of input (e.g. pointer motion reported to
/// an application that asked for every movement).
const OUTBOUND_QUEUE: usize = 1024;

/// How long the connection may be quiet before the client pings the daemon.
pub const PING_INTERVAL: Duration = Duration::from_secs(5);
/// How long the connection may go without a single frame from the daemon, pings
/// included, before it counts as dead. Generous: a daemon busy for seconds is still
/// alive, and reconnecting re-attaches every pane, which costs it more.
pub const LIVENESS_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest connecting and the handshake may take together.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The request id pings use. Real requests count from 1, and the reader drops answers
/// nobody waits for, so a ping's answer only counts as hearing from the daemon.
const PING_ID: u64 = 0;

type Pending = Mutex<HashMap<u64, oneshot::Sender<Outcome>>>;

/// A connected, handshaken client. Cheap to clone; all clones share one connection.
#[derive(Clone)]
pub struct Client {
    out: mpsc::Sender<v1::Envelope>,
    pending: Arc<Pending>,
    next_id: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    /// Becomes `true` once the connection has ended, for whatever reason.
    closed: watch::Receiver<bool>,
    welcome: Arc<v1::Welcome>,
}

/// Unsolicited events from the daemon. The stream ends when the connection closes.
pub type Events = mpsc::Receiver<event::Body>;

impl Client {
    /// Connects to `endpoint` and performs the handshake.
    pub async fn connect(
        endpoint: &Endpoint,
        client_name: &str,
    ) -> Result<(Self, Events), ClientError> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, platform::connect(endpoint))
            .await
            .map_err(|_| ClientError::TimedOut("connecting"))??;
        Self::over(stream, client_name).await
    }

    /// Runs the protocol over an already-open stream (useful for tests and tunnels).
    pub async fn over<S>(stream: S, client_name: &str) -> Result<(Self, Events), ClientError>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let welcome = tokio::time::timeout(
            CONNECT_TIMEOUT,
            handshake(&mut reader, &mut writer, client_name),
        )
        .await
        .map_err(|_| ClientError::TimedOut("the handshake"))??;

        let (closed_tx, closed) = watch::channel(false);
        let closed_tx = Arc::new(closed_tx);
        let last_heard = Arc::new(Mutex::new(Instant::now()));

        let (out, mut outbound) = mpsc::channel::<v1::Envelope>(OUTBOUND_QUEUE);
        let writer_closed = closed_tx.clone();
        let mut writer_stop = closed.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    frame = outbound.recv() => frame,
                    _ = writer_stop.wait_for(|closed| *closed) => None,
                };
                // `None`: every `Client` was dropped, or the connection ended.
                let Some(frame) = frame else { break };
                if let Err(error) = write_envelope(&mut writer, &frame).await {
                    tracing::info!(%error, "writing to the daemon failed; closing the connection");
                    break;
                }
            }
            writer_closed.send_replace(true);
        });

        tokio::spawn(heartbeat(
            out.downgrade(),
            last_heard.clone(),
            closed_tx.clone(),
            closed.clone(),
        ));

        let pending: Arc<Pending> = Arc::default();
        let (events_tx, events) = mpsc::channel(EVENT_QUEUE);
        let reader_pending = pending.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let reader_alive = alive.clone();
        let mut reader_stop = closed.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    frame = read_envelope(&mut reader) => frame,
                    _ = reader_stop.wait_for(|closed| *closed) => break,
                };
                *last_heard.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
                match frame {
                    Ok(Some(v1::Envelope {
                        kind:
                            Some(Kind::Response(v1::Response {
                                id,
                                outcome: Some(outcome),
                            })),
                    })) => {
                        let waiter = reader_pending
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .remove(&id);
                        if let Some(waiter) = waiter {
                            let _ = waiter.send(outcome);
                        }
                    }
                    Ok(Some(v1::Envelope {
                        kind: Some(Kind::Event(v1::Event { body: Some(body) })),
                    })) => {
                        if events_tx.send(body).await.is_err() {
                            // Nobody listens for events; keep serving responses.
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(error) => {
                        tracing::info!(%error, "reading from the daemon failed");
                        break;
                    }
                }
            }
            // Dropping the senders fails every in-flight request with `Disconnected`,
            // and dropping `events_tx` (and the read half) ends the stream and the socket.
            reader_alive.store(false, Ordering::SeqCst);
            closed_tx.send_replace(true);
            reader_pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
        });

        Ok((
            Self {
                out,
                pending,
                next_id: Arc::new(AtomicU64::new(1)),
                alive,
                closed,
                welcome: Arc::new(welcome),
            },
            events,
        ))
    }

    /// The daemon's handshake reply (version, host name).
    pub fn welcome(&self) -> &v1::Welcome {
        &self.welcome
    }

    /// Sends a request and waits for its outcome. Error outcomes become `Err`.
    pub async fn request(&self, body: request::Body) -> Result<Outcome, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        if !self.alive.load(Ordering::SeqCst)
            || self.out.send(envelope::request(id, body)).await.is_err()
        {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            return Err(ClientError::Disconnected);
        }
        match rx.await.map_err(|_| ClientError::Disconnected)? {
            Outcome::Error(e) => Err(ClientError::from_wire(&e)),
            outcome => Ok(outcome),
        }
    }

    /// Sends a fire-and-forget notification.
    pub async fn notify(&self, body: notification::Body) -> Result<(), ClientError> {
        self.out
            .send(envelope::notification(body))
            .await
            .map_err(|_| ClientError::Disconnected)
    }

    /// Queues a notification without waiting. Notifications queued from one thread are
    /// sent in the order they were queued, which input needs: a mouse release must never
    /// overtake its press. Fails if the connection is closed or its queue is full.
    pub fn try_notify(&self, body: notification::Body) -> Result<(), ClientError> {
        self.out
            .try_send(envelope::notification(body))
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => {
                    ClientError::Protocol("outgoing queue is full".into())
                }
                mpsc::error::TrySendError::Closed(_) => ClientError::Disconnected,
            })
    }

    /// Whether the connection is still open.
    pub fn is_connected(&self) -> bool {
        self.alive.load(Ordering::SeqCst) && !self.out.is_closed() && !*self.closed.borrow()
    }
}

/// Sends `Hello` and reads the daemon's `Welcome`.
async fn handshake<R, W>(
    reader: &mut R,
    writer: &mut W,
    client_name: &str,
) -> Result<v1::Welcome, ClientError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    write_envelope(
        writer,
        &envelope::hello(0, client_name, env!("CARGO_PKG_VERSION")),
    )
    .await?;
    match read_envelope(reader).await? {
        Some(v1::Envelope {
            kind:
                Some(Kind::Response(v1::Response {
                    outcome: Some(outcome),
                    ..
                })),
        }) => match outcome {
            Outcome::Welcome(w) => Ok(w),
            Outcome::Error(e) => Err(ClientError::from_wire(&e)),
            _ => Err(ClientError::Protocol(
                "unexpected handshake response".into(),
            )),
        },
        _ => Err(ClientError::Protocol(
            "daemon closed the connection during handshake".into(),
        )),
    }
}

/// Pings the daemon whenever the connection has been quiet for [`PING_INTERVAL`], and
/// ends the connection once nothing has arrived for [`LIVENESS_TIMEOUT`]. Holds only a
/// weak sender, so it never keeps a connection open that every `Client` has dropped.
async fn heartbeat(
    out: mpsc::WeakSender<v1::Envelope>,
    last_heard: Arc<Mutex<Instant>>,
    close: Arc<watch::Sender<bool>>,
    mut closed: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(PING_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = closed.wait_for(|closed| *closed) => return,
        }
        let quiet = last_heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .elapsed();
        if quiet >= LIVENESS_TIMEOUT {
            tracing::warn!(
                quiet_secs = quiet.as_secs(),
                "the daemon stopped answering; closing the connection"
            );
            close.send_replace(true);
            return;
        }
        let Some(out) = out.upgrade() else {
            return;
        };
        if quiet >= PING_INTERVAL {
            // A full queue already has frames on the way out; skipping a ping is fine.
            let _ = out.try_send(envelope::request(
                PING_ID,
                request::Body::Ping(v1::PingRequest {}),
            ));
        }
    }
}
