//! Long-term X25519 keys: each daemon and each paired device has one.

use std::fmt;

use super::{LanError, from_hex, to_hex};

/// Key length for X25519.
pub(crate) const KEY_LEN: usize = 32;

/// A public key. Not secret: the daemon lists paired devices by it, and a device pins
/// its host's.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; KEY_LEN]);

impl PublicKey {
    /// Wraps raw key bytes.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// From a key received in a handshake. Fails unless it is exactly 32 bytes.
    pub(crate) fn from_slice(bytes: &[u8]) -> Result<Self, LanError> {
        bytes.try_into().map(Self).map_err(|_| LanError::BadKey)
    }

    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// Lowercase hex, for storing.
    pub fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    /// Parses [`Self::to_hex`]'s output.
    pub fn from_hex(text: &str) -> Result<Self, LanError> {
        from_hex::<KEY_LEN>(text).map(Self).ok_or(LanError::BadKey)
    }

    /// A short form people can compare between two screens, e.g. `3f2a-91c0-77de-0b14`.
    pub fn fingerprint(&self) -> String {
        let hex = to_hex(&self.0[..8]);
        hex.as_bytes()
            .chunks(4)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.fingerprint())
    }
}

/// A private key and its public key. Its `Debug` output never shows the private key.
#[derive(Clone)]
pub struct Keypair {
    private: [u8; KEY_LEN],
    public: PublicKey,
}

impl Keypair {
    /// A new random keypair.
    pub fn generate() -> Result<Self, LanError> {
        let pair = snow::Builder::new(super::handshake::connect_params()?)
            .generate_keypair()
            .map_err(|e| LanError::Random(e.to_string()))?;
        Ok(Self {
            private: pair
                .private
                .as_slice()
                .try_into()
                .map_err(|_| LanError::BadKey)?,
            public: PublicKey::from_slice(&pair.public)?,
        })
    }

    /// A stored keypair, as written by [`Self::private_hex`] and [`PublicKey::to_hex`].
    pub fn from_hex(private: &str, public: &str) -> Result<Self, LanError> {
        Ok(Self {
            private: from_hex::<KEY_LEN>(private).ok_or(LanError::BadKey)?,
            public: PublicKey::from_hex(public)?,
        })
    }

    /// The private key as hex, for a private file.
    pub fn private_hex(&self) -> String {
        to_hex(&self.private)
    }

    /// The public half.
    pub fn public(&self) -> PublicKey {
        self.public
    }

    pub(crate) fn private(&self) -> &[u8; KEY_LEN] {
        &self.private
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}
