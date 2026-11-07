use std::process::Command;

use tokio::net::UnixStream;
use urushi_proto::endpoint::LocalEndpoint;

use super::Stream;

pub(crate) async fn connect_local(local: &LocalEndpoint) -> std::io::Result<Box<dyn Stream>> {
    match local {
        LocalEndpoint::UnixSocket(path) => Ok(Box::new(UnixStream::connect(path).await?)),
        LocalEndpoint::NamedPipe(_) => Err(super::unsupported(local)),
    }
}

/// Puts the child in its own process group, so closing the launching terminal (or
/// quitting the app) doesn't take the daemon down with it.
pub(crate) fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}
