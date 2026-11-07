use std::process::Command;
use std::time::Duration;

use tokio::net::windows::named_pipe::ClientOptions;
use urushi_proto::endpoint::LocalEndpoint;

use super::Stream;

/// `ERROR_PIPE_BUSY`: every pipe instance is in use; retry shortly.
const ERROR_PIPE_BUSY: i32 = 231;

pub(crate) async fn connect_local(local: &LocalEndpoint) -> std::io::Result<Box<dyn Stream>> {
    let LocalEndpoint::NamedPipe(name) = local else {
        return Err(super::unsupported(local));
    };
    loop {
        match ClientOptions::new().open(name) {
            Ok(client) => return Ok(Box::new(client)),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            Err(e) => return Err(e),
        }
    }
}

/// Detaches the child from the launching console.
pub(crate) fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}
