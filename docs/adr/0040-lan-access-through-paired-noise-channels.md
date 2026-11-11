# 0040. Allow LAN access only from paired devices, through Noise, on private addresses

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
Remote access used to work only over Tailscale (ADR 0005). Tailscale tells the daemon who each peer is (`whois`), and WireGuard encrypts the traffic. Some users reach their machines another way: a home LAN, a WireGuard VPN into it (a UniFi Dream Machine, DDNS plus WireGuard), or just the same Wi-Fi. They wanted an opt-in setting, off by default, that **never opens a port or does anything dangerous**.

A LAN provides neither identity nor encryption. A laptop on café or hotel Wi-Fi shares its "LAN" with strangers, and a Urushi connection is a shell on the host. So LAN access must authenticate every device and encrypt everything itself, with nothing to configure beyond pairing.

## Decision
- **Off by default.** With `lan.enabled` off nothing listens, and no key file exists until LAN access is first turned on.
- **Private addresses only.** The daemon binds each of this machine's private addresses: RFC 1918, and IPv6 unique-local except Tailscale's `fd7a:115c:a1e0::/48`. It never binds `0.0.0.0`, `::` or a public address. It accepts connections only from private or loopback peers. It re-checks addresses every 30 s. It never touches a router or firewall: no UPnP, no NAT-PMP, no port forwarding, no firewall rules. The default port is 7421.
- **Per-device pairing.** The host shows a one-time code: 100 random bits, written as 20 Crockford base32 symbols. A code works once, for 10 minutes, and is withdrawn after 5 wrong attempts. Pairing runs `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` with the code as the pre-shared key. Both sides prove they know it and swap long-term X25519 keys. The daemon records each device: id, name, key. A device pins the host's key.
- **Encrypted connections.** Paired devices connect with `Noise_IK_25519_ChaChaPoly_BLAKE2s`. The daemon answers only keys of paired devices. The unchanged protocol (`Hello`, frames) runs inside the encrypted stream. That layer is `urushi_proto::lan`, used by both the daemon and the client.
- **Revocable.** Revoking a device, or turning LAN access off, ends its live connections at once.
- **Only the host widens access.** Turning LAN access on or off, changing its port, showing a pairing code and revoking devices are refused (`PermissionDenied`) unless they come from the host's local socket. A paired phone can't pair more devices.
- **Bounded handshakes.** At most 8 handshakes run at once, each limited to 10 s.
- **Keys stay private.** The daemon keeps its key and the device list in `<home>/lan.json`. Each client keeps its key and paired hosts in `lan-keyring.json` in its data dir. Both files are 0600. Clients only ever see fingerprints.

## Alternatives considered
- **A shared access key** (one secret for every device): simpler, but revoking one lost phone means re-keying every device. Rejected after asking the user.
- **A token over plain TCP:** the token and every keystroke could be sniffed on any shared network. Not acceptable for a remote shell.
- **TLS (rustls) with self-signed certificates and pinning:** needs `ring` or `aws-lc-rs` (C and assembly, harder for iOS and Android builds), a certificate format, and a separate client-authentication scheme. Noise gives mutual key authentication in one small, pure-Rust crate.
- **A short code with a PAKE (e.g. SPAKE2):** would allow 6-digit codes, but adds a second protocol. A 100-bit code is long, but it is typed once per device, and it is safe even against offline guessing from a recorded handshake.
- **UPnP or NAT-PMP so it works from outside:** this is exactly the "opening ports" the user ruled out. A VPN into the LAN covers that use without exposing the daemon.
- **Binding `0.0.0.0` and filtering peers:** it would also listen on public and tailnet interfaces. Binding specific private addresses is safer.

## Consequences
- New dependencies:
  - `snow`, with RustCrypto ChaCha20-Poly1305, BLAKE2 and curve25519-dalek;
  - `getrandom`;
  - `if-addrs` (daemon only).

  All are permissive and pure Rust, and they build for iOS and Android. `snow`'s `std` feature is left off, because it would pull in `ring`.
- The daemon owns a new long-term secret. The client keeps per-device keyrings, which on mobile live in the app's private storage, not the OS keychain.
- The connect dialog gains a pairing-code field and a list of paired hosts. Settings gains a LAN section on the host.
- A device remembers a host by the address it paired with. If the host's LAN address changes, the device must pair again. A stable name (e.g. `mac.local`) or a DHCP reservation avoids that.
- The OS may ask once whether `urushid` may accept incoming connections. That is the OS's firewall asking; Urushi never changes it.
