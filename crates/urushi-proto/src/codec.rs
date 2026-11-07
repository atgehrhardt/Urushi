//! Length-prefixed framing: each frame is a `u32` big-endian length followed by one
//! encoded [`Envelope`].
//!
//! [`FrameDecoder`] works with any byte source (sync, async, or a test buffer). The
//! `io` feature adds [`read_envelope`] and [`write_envelope`] for tokio streams.

use prost::Message;
use thiserror::Error;

use crate::v1::Envelope;

/// Largest frame accepted or produced. Bigger frames close the connection, so a buggy
/// or malicious peer can't make us allocate without limit.
pub const MAX_FRAME_LEN: usize = 8 * 1024 * 1024;

const HEADER_LEN: usize = 4;

/// Framing failures.
#[derive(Debug, Error)]
pub enum FrameError {
    /// A frame exceeded [`MAX_FRAME_LEN`].
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_LEN} byte limit")]
    TooLarge(usize),
    /// The payload wasn't a valid `Envelope`.
    #[error("malformed frame: {0}")]
    Decode(#[from] prost::DecodeError),
    /// The stream ended in the middle of a frame.
    #[error("connection closed mid-frame")]
    Truncated,
    /// The underlying stream failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// Encodes one envelope as a complete frame (header plus payload).
pub fn encode_frame(envelope: &Envelope) -> Result<Vec<u8>, FrameError> {
    let len = envelope.encoded_len();
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    let header = u32::try_from(len).map_err(|_| FrameError::TooLarge(len))?;
    let mut out = Vec::with_capacity(HEADER_LEN + len);
    out.extend_from_slice(&header.to_be_bytes());
    envelope
        .encode(&mut out)
        .map_err(|_| FrameError::TooLarge(len))?;
    Ok(out)
}

/// Incremental decoder. Feed it bytes as they arrive, then take complete envelopes out.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    /// An empty decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends received bytes.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Returns the next complete envelope, or `None` if more bytes are needed.
    pub fn next_envelope(&mut self) -> Result<Option<Envelope>, FrameError> {
        let Some(len) = self.pending_len()? else {
            return Ok(None);
        };
        if self.buffer.len() < HEADER_LEN + len {
            return Ok(None);
        }
        let envelope = Envelope::decode(&self.buffer[HEADER_LEN..HEADER_LEN + len])?;
        self.buffer.drain(..HEADER_LEN + len);
        Ok(Some(envelope))
    }

    /// Whether bytes of an unfinished frame are buffered.
    pub fn has_partial_frame(&self) -> bool {
        !self.buffer.is_empty()
    }

    fn pending_len(&self) -> Result<Option<usize>, FrameError> {
        let Some(header) = self.buffer.first_chunk::<HEADER_LEN>() else {
            return Ok(None);
        };
        let len = u32::from_be_bytes(*header) as usize;
        if len > MAX_FRAME_LEN {
            return Err(FrameError::TooLarge(len));
        }
        Ok(Some(len))
    }
}

/// Reads one envelope. Returns `Ok(None)` on a clean end of stream between frames.
#[cfg(feature = "io")]
pub async fn read_envelope<R>(reader: &mut R) -> Result<Option<Envelope>, FrameError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut header = [0u8; HEADER_LEN];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            FrameError::Truncated
        } else {
            FrameError::Io(e)
        }
    })?;
    Ok(Some(Envelope::decode(payload.as_slice())?))
}

/// Writes one envelope as a frame. Does not flush.
#[cfg(feature = "io")]
pub async fn write_envelope<W>(writer: &mut W, envelope: &Envelope) -> Result<(), FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    writer.write_all(&encode_frame(envelope)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope;
    use crate::v1::request::Body;

    fn sample() -> Envelope {
        envelope::request(
            7,
            Body::ListWorkspace(crate::v1::ListWorkspaceRequest { subscribe: true }),
        )
    }

    #[test]
    fn decoder_handles_byte_at_a_time_and_multiple_frames() {
        let mut bytes = encode_frame(&sample()).unwrap();
        bytes.extend(encode_frame(&sample()).unwrap());
        let mut decoder = FrameDecoder::new();
        let mut out = Vec::new();
        for b in bytes {
            decoder.push(&[b]);
            while let Some(e) = decoder.next_envelope().unwrap() {
                out.push(e);
            }
        }
        assert_eq!(out, vec![sample(), sample()]);
        assert!(!decoder.has_partial_frame());
    }

    #[test]
    fn decoder_rejects_oversized_header() {
        let mut decoder = FrameDecoder::new();
        decoder.push(&u32::MAX.to_be_bytes());
        assert!(matches!(
            decoder.next_envelope(),
            Err(FrameError::TooLarge(_))
        ));
    }

    #[test]
    fn decoder_rejects_garbage_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.push(&3u32.to_be_bytes());
        decoder.push(&[0xff, 0xff, 0xff]);
        assert!(matches!(
            decoder.next_envelope(),
            Err(FrameError::Decode(_))
        ));
    }

    #[cfg(feature = "io")]
    #[tokio::test]
    async fn async_round_trip_and_clean_eof() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_envelope(&mut a, &sample()).await.unwrap();
        drop(a);
        assert_eq!(read_envelope(&mut b).await.unwrap(), Some(sample()));
        assert_eq!(read_envelope(&mut b).await.unwrap(), None);
    }

    #[cfg(feature = "io")]
    #[tokio::test]
    async fn async_truncated_frame_is_an_error() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let frame = encode_frame(&sample()).unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut a, &frame[..frame.len() - 1])
            .await
            .unwrap();
        drop(a);
        assert!(matches!(
            read_envelope(&mut b).await,
            Err(FrameError::Truncated)
        ));
    }
}
