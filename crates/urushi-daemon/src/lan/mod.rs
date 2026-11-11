//! LAN access (ADR 0040): paired devices connecting over this machine's private
//! addresses, through an encrypted Noise channel (`urushi_proto::lan`).
//!
//! - [`addresses`]: which addresses may be listened on and accepted from.
//! - [`identity`]: the daemon's key and the paired devices, in a private file.
//! - [`pairing`]: the one-time pairing code.
//! - `state`: what the service keeps while LAN access is on.
//!
//! The listener itself is in `server::lan`. Nothing here touches a router or firewall:
//! LAN access only listens, and only when switched on.

pub mod addresses;
pub mod identity;
pub mod pairing;
pub(crate) mod state;

pub use addresses::{LanInterfaces, StaticInterfaces, SystemInterfaces};
