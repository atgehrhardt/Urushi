//! Encrypted, paired connections over a LAN (ADR 0041).
//!
//! A LAN, unlike a tailnet, tells the daemon nothing about who is connecting and
//! doesn't encrypt anything. So a LAN connection starts with a short plaintext
//! [prelude](handshake) and a Noise handshake, and every protocol frame after that
//! travels inside a [`SecureStream`]:
//!
//! - **Pairing** (`Noise_XXpsk3`): the user reads a one-time [`PairingCode`] off the host
//!   and types it into the device. Both sides prove they know it, and swap their
//!   long-term [`PublicKey`]s.
//! - **Connecting** (`Noise_IK`): the device knows the host's key from pairing, and the
//!   host accepts only keys of devices it paired.
//!
//! The protocol inside is unchanged: `Hello`, then length-prefixed envelopes.

mod code;
mod handshake;
mod keys;
mod stream;

pub use code::PairingCode;
pub use handshake::{
    Mode, PairedDevice, PairedHost, accept_connect, accept_pair, connect, pair, read_prelude,
};
pub use keys::{Keypair, PublicKey};
pub use stream::SecureStream;

use thiserror::Error;

/// The longest device or host name carried in a pairing handshake, in bytes.
pub const MAX_NAME_LEN: usize = 64;

/// What can go wrong setting up or using a LAN connection.
#[derive(Debug, Error)]
pub enum LanError {
    /// The network failed.
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    /// The other side hung up during the handshake.
    #[error("the connection closed during the handshake")]
    Closed,
    /// The other side doesn't speak Urushi's LAN protocol.
    #[error("not an Urushi LAN connection")]
    NotUrushi,
    /// The other side speaks a version of the LAN protocol we don't.
    #[error("unsupported Urushi LAN protocol version {0}")]
    UnsupportedVersion(u8),
    /// The host didn't accept this device's key.
    #[error(
        "the host didn't accept this device: it isn't paired, was revoked, or the host's key \
         changed. Pair it again with a new code."
    )]
    Refused,
    /// The host didn't accept the pairing code.
    #[error("the host refused the pairing code: it's wrong, expired or already used")]
    PairingRefused,
    /// A pairing code that isn't 20 letters and digits.
    #[error("a pairing code has 20 letters and digits, like ABCDE-FGH12-34567-89JKM")]
    BadCode,
    /// A key that isn't 32 bytes of hex.
    #[error("invalid key")]
    BadKey,
    /// The handshake carried something unexpected.
    #[error("unexpected handshake message: {0}")]
    BadMessage(&'static str),
    /// Encryption or the handshake itself failed.
    #[error("encryption failed: {0}")]
    Noise(#[from] snow::Error),
    /// The system couldn't produce random numbers.
    #[error("no randomness available: {0}")]
    Random(String),
}

/// Lowercase hex of `bytes`.
fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

/// Exactly `N` bytes from hex (either case).
fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim().as_bytes();
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, pair) in text.chunks_exact(2).enumerate() {
        let high = char::from(pair[0]).to_digit(16)?;
        let low = char::from(pair[1]).to_digit(16)?;
        out[i] = u8::try_from(high * 16 + low).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests;
