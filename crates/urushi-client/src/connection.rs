//! A connection to a daemon: handshake, request/response matching, and the event stream.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
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

type Pending = Mutex<HashMap<u64, oneshot::Sender<Outcome>>>;

/// A connected, handshaken client. Cheap to clone; all clones share one connection.
#[derive(Clone)]
pub struct Client {
    out: mpsc::Sender<v1::Envelope>,
    pending: Arc<Pending>,
    next_id: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
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
        let stream = platform::connect(endpoint).await?;
        Self::over(stream, client_name).await
    }

    /// Runs the protocol over an already-open stream (useful for tests and tunnels).
    pub async fn over<S>(stream: S, client_name: &str) -> Result<(Self, Events), ClientError>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);
        write_envelope(
            &mut writer,
            &envelope::hello(0, client_name, env!("CARGO_PKG_VERSION")),
        )
        .await?;
        let welcome = match read_envelope(&mut reader).await? {
            Some(v1::Envelope {
                kind:
                    Some(Kind::Response(v1::Response {
                        outcome: Some(outcome),
                        ..
                    })),
            }) => match outcome {
                Outcome::Welcome(w) => w,
                Outcome::Error(e) => return Err(ClientError::from_wire(&e)),
                _ => {
                    return Err(ClientError::Protocol(
                        "unexpected handshake response".into(),
                    ));
                }
            },
            _ => {
                return Err(ClientError::Protocol(
                    "daemon closed the connection during handshake".into(),
                ));
            }
        };

        let (out, mut outbound) = mpsc::channel::<v1::Envelope>(OUTBOUND_QUEUE);
        tokio::spawn(async move {
            while let Some(frame) = outbound.recv().await {
                if write_envelope(&mut writer, &frame).await.is_err() {
                    break;
                }
            }
        });

        let pending: Arc<Pending> = Arc::default();
        let (events_tx, events) = mpsc::channel(EVENT_QUEUE);
        let reader_pending = pending.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let reader_alive = alive.clone();
        tokio::spawn(async move {
            loop {
                match read_envelope(&mut reader).await {
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
                    Ok(None) | Err(_) => break,
                }
            }
            // Dropping the senders fails every in-flight request with `Disconnected`.
            reader_alive.store(false, Ordering::SeqCst);
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
        self.alive.load(Ordering::SeqCst) && !self.out.is_closed()
    }
}
