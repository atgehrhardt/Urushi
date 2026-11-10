//! The quick terminal's request (ADR 0031). Its layout requests are the tab ones
//! ([`Client::split_pane`], [`Client::close_pane`], …) with its id as the tab id.

use urushi_proto::v1::{self, request::Body, response::Outcome};

use crate::requests::unexpected;
use crate::{Client, ClientError};

/// The first protocol minor version with the quick terminal.
pub const QUICK_TERMINAL_SINCE_MINOR: u32 = 20;

impl Client {
    /// The daemon's quick terminal, opened first (one shell in the home directory) if
    /// there is none. Needs protocol 1.20.
    pub async fn open_quick_terminal(&self) -> Result<v1::QuickTerminal, ClientError> {
        let body = Body::OpenQuickTerminal(v1::OpenQuickTerminalRequest {});
        match self.request(body).await? {
            Outcome::QuickTerminal(quick) => Ok(quick),
            _ => Err(unexpected("OpenQuickTerminal")),
        }
    }
}
