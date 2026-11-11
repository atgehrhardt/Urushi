//! The prelude and the two Noise handshakes.
//!
//! A LAN connection opens with 12 plaintext bytes: `URUSHI-LAN`, the LAN protocol
//! version, and the [`Mode`]. Handshake messages follow, each with a 2-byte big-endian
//! length. The prelude is also the handshake's prologue, so it can't be altered on the
//! way.
//!
//! - **Connect** is `Noise_IK_25519_ChaChaPoly_BLAKE2s`. The device sends its key in the
//!   first message, encrypted to the host's pinned key, and the host answers only if
//!   that key is paired.
//! - **Pair** is `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` with the pairing code as the
//!   pre-shared key. The device's last handshake message carries its name. Then the
//!   host, having recorded the device, sends a confirmation byte and its own name as
//!   the first transport message. Both names are therefore only revealed once
//!   the code has been proven.

use snow::params::NoiseParams;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::code::PairingCode;
use super::keys::{Keypair, PublicKey};
use super::stream::SecureStream;
use super::{LanError, MAX_NAME_LEN};

const MAGIC: &[u8; 10] = b"URUSHI-LAN";
const VERSION: u8 = 1;
const PRELUDE_LEN: usize = 12;
/// First byte of the host's pairing confirmation, so it is never empty.
const PAIRED: u8 = 1;
/// The largest Noise message.
pub(crate) const MAX_MESSAGE: usize = 65_535;
/// The authentication tag Noise adds to every encrypted payload.
pub(crate) const TAG_LEN: usize = 16;

/// What a LAN connection is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A paired device connecting to use the daemon.
    Connect,
    /// A device pairing with a one-time code.
    Pair,
}

impl Mode {
    fn byte(self) -> u8 {
        match self {
            Self::Connect => 0,
            Self::Pair => 1,
        }
    }
}

/// What a device learns about its host when pairing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedHost {
    /// The host's key, to pin.
    pub key: PublicKey,
    /// The host's name, for showing.
    pub host_name: String,
}

/// What a host learns about a device when pairing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedDevice {
    /// The device's key, to accept from now on.
    pub key: PublicKey,
    /// The name the device gave, for listing (at most [`MAX_NAME_LEN`] bytes).
    pub name: String,
}

pub(crate) fn connect_params() -> Result<NoiseParams, LanError> {
    Ok("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?)
}

fn pair_params() -> Result<NoiseParams, LanError> {
    Ok("Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s".parse()?)
}

fn prelude(mode: Mode) -> [u8; PRELUDE_LEN] {
    let mut out = [0u8; PRELUDE_LEN];
    out[..MAGIC.len()].copy_from_slice(MAGIC);
    out[MAGIC.len()] = VERSION;
    out[MAGIC.len() + 1] = mode.byte();
    out
}

/// Reads the prelude a device sent, on the host.
pub async fn read_prelude<S>(stream: &mut S) -> Result<Mode, LanError>
where
    S: AsyncRead + Unpin,
{
    let mut bytes = [0u8; PRELUDE_LEN];
    stream.read_exact(&mut bytes).await.map_err(closed)?;
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(LanError::NotUrushi);
    }
    if bytes[MAGIC.len()] != VERSION {
        return Err(LanError::UnsupportedVersion(bytes[MAGIC.len()]));
    }
    match bytes[MAGIC.len() + 1] {
        0 => Ok(Mode::Connect),
        1 => Ok(Mode::Pair),
        _ => Err(LanError::BadMessage("unknown mode")),
    }
}

/// Connects a paired device to its host. `host` is the key pinned when pairing.
pub async fn connect<S>(
    mut stream: S,
    me: &Keypair,
    host: &PublicKey,
) -> Result<SecureStream<S>, LanError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let prologue = prelude(Mode::Connect);
    let mut noise = snow::Builder::new(connect_params()?)
        .prologue(&prologue)?
        .local_private_key(me.private())?
        .remote_public_key(host.as_bytes())?
        .build_initiator()?;
    stream.write_all(&prologue).await?;
    // A host that doesn't accept this key hangs up instead of answering, which the
    // device may meet while still writing.
    send(&mut stream, &mut noise, &[]).await.map_err(refused)?;
    let reply = receive_raw(&mut stream).await.map_err(refused)?;
    let mut payload = vec![0u8; MAX_MESSAGE];
    noise
        .read_message(&reply, &mut payload)
        .map_err(|_| LanError::Refused)?;
    Ok(SecureStream::new(stream, noise.into_transport_mode()?))
}

/// Answers a device that sent [`Mode::Connect`], once the prelude has been read.
/// `is_paired` decides whether its key is accepted; if not, the host hangs up without
/// answering. Returns the stream and the device's key.
pub async fn accept_connect<S>(
    mut stream: S,
    me: &Keypair,
    is_paired: impl FnOnce(&PublicKey) -> bool,
) -> Result<(SecureStream<S>, PublicKey), LanError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut noise = snow::Builder::new(connect_params()?)
        .prologue(&prelude(Mode::Connect))?
        .local_private_key(me.private())?
        .build_responder()?;
    receive(&mut stream, &mut noise).await?;
    let key = PublicKey::from_slice(
        noise
            .get_remote_static()
            .ok_or(LanError::BadMessage("no device key"))?,
    )?;
    if !is_paired(&key) {
        return Err(LanError::Refused);
    }
    send(&mut stream, &mut noise, &[]).await?;
    Ok((SecureStream::new(stream, noise.into_transport_mode()?), key))
}

/// Pairs this device with a host, using the code shown on the host.
pub async fn pair<S>(
    mut stream: S,
    me: &Keypair,
    code: PairingCode,
    device_name: &str,
) -> Result<PairedHost, LanError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let prologue = prelude(Mode::Pair);
    let psk = code.psk();
    let mut noise = snow::Builder::new(pair_params()?)
        .prologue(&prologue)?
        .local_private_key(me.private())?
        .psk(3, &psk)?
        .build_initiator()?;
    stream.write_all(&prologue).await?;
    // A host that isn't pairing hangs up after the prelude, which the device may meet
    // while writing or while reading.
    send(&mut stream, &mut noise, &[])
        .await
        .map_err(refused_pairing)?;
    receive(&mut stream, &mut noise)
        .await
        .map_err(refused_pairing)?;
    send(&mut stream, &mut noise, truncate(device_name).as_bytes())
        .await
        .map_err(refused_pairing)?;
    let key = PublicKey::from_slice(
        noise
            .get_remote_static()
            .ok_or(LanError::BadMessage("no host key"))?,
    )?;
    let mut secure = SecureStream::new(stream, noise.into_transport_mode()?);
    // The host checks the code with our last message, and hangs up if it was wrong.
    let confirmation = secure.receive_message().await.map_err(refused_pairing)?;
    let Some((&PAIRED, host_name)) = confirmation.split_first() else {
        return Err(LanError::BadMessage("no pairing confirmation"));
    };
    Ok(PairedHost {
        key,
        host_name: String::from_utf8_lossy(host_name).into_owned(),
    })
}

/// Answers a device that sent [`Mode::Pair`], once the prelude has been read. Fails if
/// the device didn't prove it knows `code`. Otherwise `register` records the device,
/// and the host confirms by sending its name.
pub async fn accept_pair<S>(
    mut stream: S,
    me: &Keypair,
    code: PairingCode,
    host_name: &str,
    register: impl FnOnce(&PairedDevice) -> Result<(), LanError>,
) -> Result<PairedDevice, LanError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let psk = code.psk();
    let mut noise = snow::Builder::new(pair_params()?)
        .prologue(&prelude(Mode::Pair))?
        .local_private_key(me.private())?
        .psk(3, &psk)?
        .build_responder()?;
    receive(&mut stream, &mut noise).await?;
    send(&mut stream, &mut noise, &[]).await?;
    let name = receive(&mut stream, &mut noise)
        .await
        .map_err(|e| match e {
            LanError::Noise(_) => LanError::PairingRefused,
            e => e,
        })?;
    let device = PairedDevice {
        key: PublicKey::from_slice(
            noise
                .get_remote_static()
                .ok_or(LanError::BadMessage("no device key"))?,
        )?,
        name: truncate(&String::from_utf8_lossy(&name)).to_owned(),
    };
    register(&device)?;
    let mut secure = SecureStream::new(stream, noise.into_transport_mode()?);
    let mut confirmation = vec![PAIRED];
    confirmation.extend_from_slice(truncate(host_name).as_bytes());
    secure.send_message(&confirmation).await?;
    Ok(device)
}

/// At most [`MAX_NAME_LEN`] bytes of `name`, cut at a character boundary.
fn truncate(name: &str) -> &str {
    let name = name.trim();
    let mut end = name.len().min(MAX_NAME_LEN);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// A host hanging up during a connect means it refused this device.
fn refused(e: LanError) -> LanError {
    match e {
        LanError::Closed => LanError::Refused,
        e => e,
    }
}

fn refused_pairing(e: LanError) -> LanError {
    match e {
        LanError::Closed | LanError::Noise(_) => LanError::PairingRefused,
        e => e,
    }
}

async fn send<S>(
    stream: &mut S,
    noise: &mut snow::HandshakeState,
    payload: &[u8],
) -> Result<(), LanError>
where
    S: AsyncWrite + Unpin,
{
    let mut message = vec![0u8; MAX_MESSAGE];
    let len = noise.write_message(payload, &mut message)?;
    let header = u16::try_from(len).map_err(|_| LanError::BadMessage("message too long"))?;
    stream
        .write_all(&header.to_be_bytes())
        .await
        .map_err(closed)?;
    stream.write_all(&message[..len]).await.map_err(closed)?;
    stream.flush().await.map_err(closed)?;
    Ok(())
}

async fn receive<S>(stream: &mut S, noise: &mut snow::HandshakeState) -> Result<Vec<u8>, LanError>
where
    S: AsyncRead + Unpin,
{
    let message = receive_raw(stream).await?;
    let mut payload = vec![0u8; MAX_MESSAGE];
    let len = noise.read_message(&message, &mut payload)?;
    payload.truncate(len);
    Ok(payload)
}

async fn receive_raw<S>(stream: &mut S) -> Result<Vec<u8>, LanError>
where
    S: AsyncRead + Unpin,
{
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).await.map_err(closed)?;
    let mut message = vec![0u8; usize::from(u16::from_be_bytes(header))];
    stream.read_exact(&mut message).await.map_err(closed)?;
    Ok(message)
}

fn closed(e: std::io::Error) -> LanError {
    use std::io::ErrorKind;
    // A peer that hangs up shows as the end of the stream, or, depending on the OS and
    // on whether we were reading or writing, as a reset, an abort or a broken pipe.
    if matches!(
        e.kind(),
        ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
    ) {
        LanError::Closed
    } else {
        LanError::Io(e)
    }
}
