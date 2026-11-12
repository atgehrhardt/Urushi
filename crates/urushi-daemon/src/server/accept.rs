//! Accept loops that outlive their errors.
//!
//! A listener's `accept` fails for reasons that say nothing about the listener: the
//! process is out of file descriptors (`EMFILE`), a peer reset the connection before
//! it was accepted, the system is short of memory. None of them may stop the daemon,
//! which would close every terminal (UR-69). Each failure is logged, the loop waits a
//! moment so it can't spin, and accepting carries on.

use std::future::Future;
use std::ops::ControlFlow;
use std::time::Duration;

/// How long a loop waits after a failed `accept` before trying again. Long enough not
/// to spin while descriptors are exhausted, short enough that clients barely notice.
pub(crate) const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// Accepts connections forever: each one goes to `handle`, which may end the loop with
/// `ControlFlow::Break` (the daemon is going away). A failed accept is logged under
/// `listener` and retried after [`ACCEPT_RETRY`]; it never ends the loop.
pub(crate) async fn accept_forever<T, A, F>(
    listener: &'static str,
    mut accept: A,
    mut handle: impl FnMut(T) -> ControlFlow<()>,
) where
    A: FnMut() -> F,
    F: Future<Output = std::io::Result<T>>,
{
    loop {
        match accept().await {
            Ok(accepted) => {
                if handle(accepted).is_break() {
                    return;
                }
            }
            Err(error) => {
                tracing::warn!(listener, %error, "accepting a connection failed; retrying");
                tokio::time::sleep(ACCEPT_RETRY).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn an_accept_error_is_retried_and_the_next_connection_served() {
        let results = Arc::new(Mutex::new(VecDeque::from([
            Err(std::io::Error::from_raw_os_error(24)), // EMFILE
            Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted)),
            Ok(7),
            Ok(8),
        ])));
        let mut served = Vec::new();
        let started = tokio::time::Instant::now();
        accept_forever(
            "test",
            || {
                let next = results.lock().unwrap().pop_front();
                async move { next.unwrap_or(Ok(0)) }
            },
            |n| {
                served.push(n);
                if n == 8 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        )
        .await;
        assert_eq!(served, [7, 8]);
        // It waited after each failure instead of spinning.
        assert!(started.elapsed() >= ACCEPT_RETRY * 2);
    }
}
