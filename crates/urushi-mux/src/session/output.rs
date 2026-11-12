//! PTY output on its way from the reader thread to the actor, taken in batches.
//!
//! The reader thread sends what each `read` returned, often a few KiB. Feeding chunks
//! that are already queued to the emulator together, and publishing one new version for
//! all of them, means a busy pane wakes its watchers (screen streams, attention) once per
//! batch rather than once per read (UR-69).

use tokio::sync::mpsc;

/// Most output bytes taken into one batch.
pub(super) const OUTPUT_BATCH: usize = 1024 * 1024;

/// What the reader thread sends.
pub(super) enum Output {
    /// Bytes the process wrote.
    Bytes(Vec<u8>),
    /// The PTY closed: the process (and everything holding its terminal) has gone.
    Closed,
}

/// `first`, plus the output chunks already queued behind it, up to [`OUTPUT_BATCH`]
/// bytes. Also says whether the PTY closed among them.
pub(super) fn take_queued(
    first: Vec<u8>,
    output: &mut mpsc::Receiver<Output>,
) -> (Vec<Vec<u8>>, bool) {
    let mut size = first.len();
    let mut batch = vec![first];
    while size < OUTPUT_BATCH {
        match output.try_recv() {
            Ok(Output::Bytes(bytes)) => {
                size += bytes.len();
                batch.push(bytes);
            }
            Ok(Output::Closed) => return (batch, true),
            Err(_) => break,
        }
    }
    (batch, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_chunks_are_taken_together() {
        let (tx, mut rx) = mpsc::channel(128);
        for i in 0..100u8 {
            tx.try_send(Output::Bytes(vec![i; 10])).unwrap();
        }
        let (batch, closed) = take_queued(vec![0; 10], &mut rx);
        assert_eq!(batch.len(), 101);
        assert!(!closed);
        assert!(rx.try_recv().is_err(), "nothing left behind");
    }

    #[test]
    fn a_close_among_queued_chunks_is_reported() {
        let (tx, mut rx) = mpsc::channel(8);
        tx.try_send(Output::Bytes(b"bye".to_vec())).unwrap();
        tx.try_send(Output::Closed).unwrap();
        let (batch, closed) = take_queued(b"good".to_vec(), &mut rx);
        assert_eq!(batch, [b"good".to_vec(), b"bye".to_vec()]);
        assert!(closed);
    }

    #[test]
    fn a_batch_stops_at_its_size_limit() {
        let (tx, mut rx) = mpsc::channel(8);
        for _ in 0..4 {
            tx.try_send(Output::Bytes(vec![0; OUTPUT_BATCH / 2]))
                .unwrap();
        }
        let (batch, _) = take_queued(vec![0; OUTPUT_BATCH / 2], &mut rx);
        assert_eq!(batch.len(), 2);
        assert!(rx.try_recv().is_ok(), "the rest waits for the next batch");
    }
}
