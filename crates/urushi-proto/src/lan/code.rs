//! One-time pairing codes.
//!
//! A code is 100 random bits written as 20 Crockford base32 symbols in groups of five
//! (`ABCDE-FGH12-34567-89JKM`). It is the pre-shared key of the pairing handshake, so it
//! must be hard to guess even offline, from a recorded handshake: 100 bits is.

use std::fmt;

use super::LanError;

/// Crockford's base32: no I, L, O or U, so nothing reads as something else.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SYMBOLS: usize = 20;
const BITS: u32 = 100;
/// Prefixed to the code to make the 32-byte pre-shared key, so a code can't be reused as
/// a key anywhere else.
const PSK_PREFIX: &[u8; 16] = b"urushi-lan-pair1";

/// A one-time pairing code. `Display` writes it grouped; `Debug` hides it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PairingCode(u128);

impl PairingCode {
    /// A new random code.
    pub fn generate() -> Result<Self, LanError> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| LanError::Random(e.to_string()))?;
        Ok(Self(u128::from_be_bytes(bytes) & mask()))
    }

    /// Parses a code as a person typed it: case, spaces and dashes don't matter, and
    /// `O`, `I` and `L` are read as `0`, `1` and `1`.
    pub fn parse(text: &str) -> Result<Self, LanError> {
        let mut value: u128 = 0;
        let mut count = 0;
        for c in text.chars() {
            if c == '-' || c.is_whitespace() {
                continue;
            }
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            let digit = ALPHABET
                .iter()
                .position(|&a| char::from(a) == c)
                .ok_or(LanError::BadCode)?;
            count += 1;
            if count > SYMBOLS {
                return Err(LanError::BadCode);
            }
            value = (value << 5) | digit as u128;
        }
        if count == SYMBOLS {
            Ok(Self(value))
        } else {
            Err(LanError::BadCode)
        }
    }

    /// The handshake's pre-shared key.
    pub(crate) fn psk(self) -> [u8; 32] {
        let mut psk = [0u8; 32];
        psk[..16].copy_from_slice(PSK_PREFIX);
        psk[16..].copy_from_slice(&self.0.to_be_bytes());
        psk
    }
}

fn mask() -> u128 {
    (1u128 << BITS) - 1
}

impl fmt::Display for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in 0..SYMBOLS {
            if i > 0 && i % 5 == 0 {
                f.write_str("-")?;
            }
            let shift = 5 * (SYMBOLS - 1 - i);
            let digit = usize::try_from((self.0 >> shift) & 0x1f).unwrap_or(0);
            write!(f, "{}", char::from(ALPHABET[digit]))?;
        }
        Ok(())
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingCode(..)")
    }
}
