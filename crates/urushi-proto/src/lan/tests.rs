use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

use super::*;
use crate::codec::{read_envelope, write_envelope};
use crate::envelope;
use crate::v1::request::Body;

fn keys() -> Keypair {
    Keypair::generate().unwrap()
}

#[test]
fn codes_round_trip_and_forgive_typing() {
    let code = PairingCode::generate().unwrap();
    let text = code.to_string();
    assert_eq!(text.len(), 23);
    assert_eq!(text.matches('-').count(), 3);
    assert_eq!(PairingCode::parse(&text).unwrap(), code);
    let sloppy = text.to_lowercase().replace('-', " ").replace('0', "o");
    assert_eq!(PairingCode::parse(&sloppy).unwrap(), code);
    assert_eq!(
        PairingCode::parse("1111l-iiiii-00000-ooooo").unwrap(),
        PairingCode::parse("11111-11111-00000-00000").unwrap()
    );
}

#[test]
fn bad_codes_are_refused() {
    for text in [
        "",
        "ABCDE",
        "ABCDE-FGH12-34567-89JK",
        "ABCDE-FGH12-34567-89JKMN",
        "UUUUU-UUUUU-UUUUU-UUUUU",
    ] {
        assert!(
            matches!(PairingCode::parse(text), Err(LanError::BadCode)),
            "{text}"
        );
    }
}

#[test]
fn codes_and_private_keys_stay_out_of_debug_output() {
    let code = PairingCode::generate().unwrap();
    assert_eq!(format!("{code:?}"), "PairingCode(..)");
    let pair = keys();
    assert!(!format!("{pair:?}").contains(&pair.private_hex()));
}

#[test]
fn keys_round_trip_as_hex() {
    let pair = keys();
    let back = Keypair::from_hex(&pair.private_hex(), &pair.public().to_hex()).unwrap();
    assert_eq!(back.public(), pair.public());
    assert_eq!(back.private_hex(), pair.private_hex());
    assert!(PublicKey::from_hex("abcd").is_err());
    assert_eq!(pair.public().fingerprint().len(), 19);
}

async fn host_pairs(
    stream: tokio::io::DuplexStream,
    host: Keypair,
    code: PairingCode,
) -> Result<PairedDevice, LanError> {
    let mut stream = stream;
    assert_eq!(read_prelude(&mut stream).await?, Mode::Pair);
    accept_pair(stream, &host, code, "my-mac", |_| Ok(())).await
}

#[tokio::test]
async fn pairing_swaps_keys_and_names() {
    let (host, device) = (keys(), keys());
    let code = PairingCode::generate().unwrap();
    let (a, b) = duplex(4096);
    let host_side = tokio::spawn(host_pairs(b, host.clone(), code));
    let paired = pair(a, &device, code, "  phone  ").await.unwrap();
    let registered = host_side.await.unwrap().unwrap();
    assert_eq!(paired.key, host.public());
    assert_eq!(paired.host_name, "my-mac");
    assert_eq!(registered.key, device.public());
    assert_eq!(registered.name, "phone");
}

#[tokio::test]
async fn pairing_with_a_wrong_code_fails_on_both_sides() {
    let (host, device) = (keys(), keys());
    let (a, b) = duplex(4096);
    let host_side = tokio::spawn(host_pairs(b, host, PairingCode::generate().unwrap()));
    let result = pair(a, &device, PairingCode::generate().unwrap(), "phone").await;
    assert!(
        matches!(result, Err(LanError::PairingRefused)),
        "{result:?}"
    );
    let host_result = host_side.await.unwrap();
    assert!(
        matches!(host_result, Err(LanError::PairingRefused)),
        "{host_result:?}"
    );
}

#[tokio::test]
async fn a_host_that_refuses_to_register_refuses_the_pairing() {
    let (host, device) = (keys(), keys());
    let code = PairingCode::generate().unwrap();
    let (a, mut b) = duplex(4096);
    let host_side = tokio::spawn(async move {
        read_prelude(&mut b).await?;
        accept_pair(b, &host, code, "my-mac", |_| {
            Err(LanError::BadMessage("full"))
        })
        .await
    });
    let result = pair(a, &device, code, "phone").await;
    assert!(
        matches!(result, Err(LanError::PairingRefused)),
        "{result:?}"
    );
    assert!(host_side.await.unwrap().is_err());
}

async fn host_accepts(
    mut stream: tokio::io::DuplexStream,
    host: Keypair,
    paired: PublicKey,
) -> Result<(SecureStream<tokio::io::DuplexStream>, PublicKey), LanError> {
    assert_eq!(read_prelude(&mut stream).await?, Mode::Connect);
    accept_connect(stream, &host, |key| *key == paired).await
}

#[tokio::test]
async fn a_paired_device_connects_and_frames_flow_both_ways() {
    let (host, device) = (keys(), keys());
    let (a, b) = duplex(1 << 20);
    let host_side = tokio::spawn(host_accepts(b, host.clone(), device.public()));
    let mut client = connect(a, &device, &host.public()).await.unwrap();
    let (mut server, key) = host_side.await.unwrap().unwrap();
    assert_eq!(key, device.public());

    let hello = envelope::hello(1, "test", "0");
    write_envelope(&mut client, &hello).await.unwrap();
    assert_eq!(read_envelope(&mut server).await.unwrap(), Some(hello));

    // Bigger than one Noise message, so it is split and joined again.
    let big = envelope::request(
        2,
        Body::ReadScreenText(crate::v1::ReadScreenTextRequest {
            pane_id: "x".repeat(200_000),
        }),
    );
    let writer = tokio::spawn(async move {
        write_envelope(&mut server, &big).await.unwrap();
        (server, big)
    });
    let received = read_envelope(&mut client).await.unwrap();
    let (mut server, big) = writer.await.unwrap();
    assert_eq!(received, Some(big));

    server.shutdown().await.unwrap();
    drop(server);
    assert_eq!(read_envelope(&mut client).await.unwrap(), None, "clean end");
}

#[tokio::test]
async fn an_unpaired_device_is_refused() {
    let (host, device, other) = (keys(), keys(), keys());
    let (a, b) = duplex(4096);
    let host_side = tokio::spawn(host_accepts(b, host.clone(), other.public()));
    let result = connect(a, &device, &host.public()).await;
    assert!(matches!(result, Err(LanError::Refused)), "{result:?}");
    assert!(matches!(host_side.await.unwrap(), Err(LanError::Refused)));
}

#[tokio::test]
async fn a_device_pinning_another_host_key_is_refused() {
    let (host, impostor, device) = (keys(), keys(), keys());
    let (a, b) = duplex(4096);
    let host_side = tokio::spawn(host_accepts(b, impostor, device.public()));
    let result = connect(a, &device, &host.public()).await;
    assert!(matches!(result, Err(LanError::Refused)), "{result:?}");
    assert!(host_side.await.unwrap().is_err());
}

#[tokio::test]
async fn tampered_ciphertext_ends_the_stream() {
    let (host, device) = (keys(), keys());
    let (a, b) = duplex(4096);
    // A relay that flips one bit of everything after the handshake.
    let (relay_device, relay_host) = duplex(4096);
    let host_side = tokio::spawn(host_accepts(b, host.clone(), device.public()));
    let device_side = tokio::spawn(async move {
        let mut client = connect(relay_device, &device, &host.public())
            .await
            .unwrap();
        client.write_all(b"secret").await.unwrap();
        client.flush().await.unwrap();
        client
    });
    let relay = tokio::spawn(async move {
        let (mut from_device, mut to_device) = tokio::io::split(relay_host);
        let (mut from_host, mut to_host) = tokio::io::split(a);
        let mut seen = 0usize;
        let up = async {
            let mut buf = [0u8; 4096];
            loop {
                let n = from_device.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                // Prelude (12) + first handshake message (2 + 96) pass untouched.
                if seen + n > 110 {
                    let i = n - 1;
                    buf[i] ^= 1;
                }
                seen += n;
                to_host.write_all(&buf[..n]).await.unwrap();
            }
        };
        let down = tokio::io::copy(&mut from_host, &mut to_device);
        let _ = tokio::join!(up, down);
    });
    let (mut server, _) = host_side.await.unwrap().unwrap();
    let _client = device_side.await.unwrap();
    let mut buf = [0u8; 16];
    let err = server.read(&mut buf).await.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    relay.abort();
}
