//! A byte stream encrypted with the keys of a finished Noise handshake.
//!
//! On the wire, bytes travel as Noise messages, each with a 2-byte big-endian length.
//! A message carries at most [`MAX_PLAINTEXT`] bytes. A tampered, replayed or reordered
//! message fails to decrypt and ends the stream with an error.
//!
//! Writes are encrypted into one pending message at a time. That message may still be
//! on its way when `poll_write` returns, so callers must flush, as
//! [`crate::codec::write_envelope`] does.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use super::LanError;
use super::handshake::{MAX_MESSAGE, TAG_LEN};

/// The most plaintext one message carries.
pub(crate) const MAX_PLAINTEXT: usize = MAX_MESSAGE - TAG_LEN;
const HEADER_LEN: usize = 2;

/// An encrypted stream. Implements tokio's `AsyncRead` and `AsyncWrite`, so the frame
/// codec runs over it unchanged.
pub struct SecureStream<S> {
    inner: S,
    noise: snow::TransportState,
    /// The incoming message being read: its header, then its ciphertext.
    incoming: Vec<u8>,
    /// Decrypted bytes not yet handed to the reader.
    plaintext: Vec<u8>,
    plaintext_read: usize,
    /// The outgoing message being written, and how much of it has been.
    outgoing: Vec<u8>,
    outgoing_written: usize,
}

impl<S> std::fmt::Debug for SecureStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureStream").finish_non_exhaustive()
    }
}

impl<S> SecureStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    pub(crate) fn new(inner: S, noise: snow::TransportState) -> Self {
        Self {
            inner,
            noise,
            incoming: Vec::new(),
            plaintext: Vec::new(),
            plaintext_read: 0,
            outgoing: Vec::new(),
            outgoing_written: 0,
        }
    }

    /// Sends `payload` as one message and flushes it (used by pairing).
    pub(crate) async fn send_message(&mut self, payload: &[u8]) -> Result<(), LanError> {
        if payload.len() > MAX_PLAINTEXT {
            return Err(LanError::BadMessage("message too long"));
        }
        self.write_all(payload).await?;
        self.flush().await?;
        Ok(())
    }

    /// Receives one whole message (used by pairing).
    pub(crate) async fn receive_message(&mut self) -> Result<Vec<u8>, LanError> {
        let mut first = [0u8; 1];
        // Reading one byte decrypts a whole message; the rest is in `plaintext`.
        let n = self.read(&mut first).await.map_err(|e| match e.kind() {
            io::ErrorKind::InvalidData => LanError::BadMessage("could not decrypt"),
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe => LanError::Closed,
            _ => LanError::Io(e),
        })?;
        if n == 0 {
            return Err(LanError::Closed);
        }
        let mut message = first.to_vec();
        message.extend_from_slice(&self.plaintext[self.plaintext_read..]);
        self.plaintext_read = self.plaintext.len();
        Ok(message)
    }

    /// Writes the pending outgoing message to `inner`.
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.outgoing_written < self.outgoing.len() {
            let n = ready!(
                Pin::new(&mut self.inner).poll_write(cx, &self.outgoing[self.outgoing_written..])
            )?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.outgoing_written += n;
        }
        self.outgoing.clear();
        self.outgoing_written = 0;
        Poll::Ready(Ok(()))
    }

    /// Bytes still missing from the incoming message (its header first).
    fn incoming_wanted(&self) -> usize {
        match self.incoming.first_chunk::<HEADER_LEN>() {
            None => HEADER_LEN - self.incoming.len(),
            Some(header) => {
                HEADER_LEN + usize::from(u16::from_be_bytes(*header)) - self.incoming.len()
            }
        }
    }
}

impl<S> AsyncRead for SecureStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if this.plaintext_read < this.plaintext.len() {
                let available = &this.plaintext[this.plaintext_read..];
                let n = available.len().min(buf.remaining());
                buf.put_slice(&available[..n]);
                this.plaintext_read += n;
                return Poll::Ready(Ok(()));
            }
            let wanted = this.incoming_wanted();
            if wanted == 0 {
                let ciphertext = &this.incoming[HEADER_LEN..];
                if ciphertext.len() < TAG_LEN {
                    return Poll::Ready(Err(invalid("message shorter than its tag")));
                }
                this.plaintext.resize(ciphertext.len(), 0);
                let len = this
                    .noise
                    .read_message(ciphertext, &mut this.plaintext)
                    .map_err(|_| invalid("message failed to decrypt"))?;
                this.plaintext.truncate(len);
                this.plaintext_read = 0;
                this.incoming.clear();
                continue;
            }
            let start = this.incoming.len();
            this.incoming.resize(start + wanted, 0);
            let mut read = ReadBuf::new(&mut this.incoming[start..]);
            let polled = Pin::new(&mut this.inner).poll_read(cx, &mut read);
            let n = read.filled().len();
            this.incoming.truncate(start + n);
            ready!(polled)?;
            if n == 0 {
                return if start == 0 {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()))
                };
            }
        }
    }
}

impl<S> AsyncWrite for SecureStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = buf.len().min(MAX_PLAINTEXT);
        this.outgoing.resize(HEADER_LEN + n + TAG_LEN, 0);
        let len = this
            .noise
            .write_message(&buf[..n], &mut this.outgoing[HEADER_LEN..])
            .map_err(|e| io::Error::other(e.to_string()))?;
        let header = u16::try_from(len).map_err(|_| invalid("message too long"))?;
        this.outgoing[..HEADER_LEN].copy_from_slice(&header.to_be_bytes());
        this.outgoing.truncate(HEADER_LEN + len);
        this.outgoing_written = 0;
        // Start sending now; whatever doesn't fit yet goes out on the next write or flush.
        if let Poll::Ready(Err(e)) = this.poll_drain(cx) {
            return Poll::Ready(Err(e));
        }
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
