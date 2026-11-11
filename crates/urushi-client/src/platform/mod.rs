//! Platform-specific transports and process detaching (ADR 0007).

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use urushi_proto::endpoint::{Endpoint, LocalEndpoint};

use crate::ClientError;

/// Any bidirectional byte stream.
pub(crate) trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

pub(crate) use imp::{daemon_pid, detach, is_running, terminate, write_private};

/// Opens a stream to `endpoint`.
pub(crate) async fn connect(endpoint: &Endpoint) -> Result<Box<dyn Stream>, ClientError> {
    let failed = |source| ClientError::Connect {
        endpoint: endpoint.to_string(),
        source,
    };
    match endpoint {
        Endpoint::Tcp { host, port } => {
            let stream = TcpStream::connect((host.as_str(), *port))
                .await
                .map_err(failed)?;
            let _ = stream.set_nodelay(true);
            Ok(Box::new(stream))
        }
        Endpoint::Local(local) => imp::connect_local(local).await.map_err(failed),
    }
}

#[allow(dead_code, reason = "used by the platform that lacks this transport")]
fn unsupported(local: &LocalEndpoint) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("{local:?} is not supported on this platform"),
    )
}
