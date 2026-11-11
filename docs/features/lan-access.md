# Feature: LAN access (paired devices, no Tailscale)

- **Status:** Shipped (daemon, CLI, desktop and mobile apps)
- **Owners (crates):** `urushi-proto` (`lan`), `urushi-daemon` (`lan/`, `server/lan.rs`), `urushi-client` (`lan.rs`), `urushi-cli` (`lan.rs`), `urushi-app` (`update/lan.rs`, `view/lan.rs`)
- **Related ADRs:** [0041](../adr/0041-lan-access-through-paired-noise-channels.md), [0005](../adr/0005-tailscale-remote.md)

## What it does
With **Settings → Allow LAN access** (or `urushi lan on`) switched on, other devices can reach the daemon on this machine's private LAN addresses. That covers the same network, or a VPN into it (WireGuard, a UniFi Dream Machine), without Tailscale. It is **off by default**. A device must first pair, using a one-time code the host shows. After that, it connects over an encrypted channel, and the host can revoke it at any time. Urushi never opens ports on a router or firewall.

## How it works
1. **Listening.** `server::lan::reconcile` runs when `Settings.lan` changes, at startup, and every 30 s from a watchdog.
   - It binds one listener per address from `LanInterfaces` (`SystemInterfaces`, via `if-addrs`).
   - That list is filtered by `lan::addresses::is_lan_address`: RFC 1918, and IPv6 ULA except Tailscale's range.
   - It never binds `0.0.0.0` or `::`. The default port is 7421.
   - The key file `<home>/lan.json` (`lan::identity::IdentityStore`, 0600) is made the first time LAN access is turned on.
2. **Accepting.** `accept_loop` drops peers that aren't private or loopback (`is_allowed_peer`). It allows at most 8 handshakes at once (`LanState.handshakes`), and each must finish within 10 s. It then reads the prelude (`urushi_proto::lan::read_prelude`).
3. **Pairing.**
   - The host asks for a code with `StartLanPairing` (desktop: **Pair a device…**; CLI: `urushi lan pair-code`). The code (`lan::pairing::Pairing`) works once, for 10 minutes, and is withdrawn after 5 wrong attempts.
   - The device runs `urushi_proto::lan::pair`. In the app, that is the connect dialog's **Pairing code** field, sent as `Effect::PairLan` to `Backend::pair_lan`; in the CLI, `urushi lan pair <host> <code>`.
   - The host's `accept_pair` checks the code and records the device with `LanState::register`. The device stores the host's key in its `LanKeyring` (`lan-keyring.json`, 0600, in its data dir).
4. **Connecting.** `Client::connect_remote` uses `connect_lan` (Noise IK) for any host in the keyring, and plain tailnet TCP for anything else.
   - The daemon's `accept_connect` answers only paired keys.
   - `LanState::connected` registers the connection with a `watch` channel, which becomes `Peer.close`.
   - The protocol then runs unchanged inside `SecureStream`, as a non-local peer.
5. **Revoking and switching off.** `RevokeLanDevice` forgets the device and drops its channels' senders, so `connection::serve` ends its connections. Turning LAN access off (`LanState::stop`) does the same for all of them and closes the listeners.

## Invariants
- **Nothing listens while LAN access is off.** No key exists before it is first used. Tests: `lan_access_is_off_until_switched_on`, `switching_lan_access_off_closes_everything`.
- **Only specific private addresses are bound,** never `0.0.0.0`, `::`, public addresses or Tailscale's ranges. Enforced by `is_lan_address`, and again by `server::lan::listenable`. Tests: `lan::addresses` tests.
- **Every LAN byte after the prelude is encrypted and authenticated.** Tampering ends the stream. Tests: `urushi_proto::lan` tests, including `tampered_ciphertext_ends_the_stream`.
- **Only paired keys get an answer.** A code works once, expires, and is withdrawn after 5 failures. Tests: `a_paired_device_connects_and_a_revoked_one_is_cut_off`, `lan::pairing` tests.
- **Only the host changes LAN access.** The `lan_*` settings, pairing and revoking are refused from remote peers, and pairing codes are sent only to local clients (`handlers::host_only`). Test: `a_lan_device_cannot_widen_lan_access`.
- **Private keys never leave their files.** Clients see fingerprints only (`PublicKey::fingerprint`), and `Debug` output hides keys and codes.

## Tests
- `crates/urushi-proto/src/lan/tests.rs`: codes, keys, pairing with right and wrong codes, IK with paired, unpaired and wrong-host keys, chunked frames, tampering.
- `crates/urushi-daemon/src/lan/*`: address rules, the identity file (0600, round trip, damaged file), pairing expiry and lockout.
- `crates/urushi-daemon/tests/lan.rs`: end to end over loopback. Covers pairing, connecting, refusals, a plain client getting the pairing hint, revoking, host-only changes, and switching off.
- `crates/urushi-client/src/lan.rs`: the keyring (written only once a host is paired, 0600, lookup with and without a port, forgetting a host).
- `crates/urushi-app/src/update/tests/lan.rs` and `view/lan.rs`: the connect dialog's pairing flow and paired hosts, and the Settings switch, code, tick refresh and revoke.

## Limitations and follow-ups
- A device remembers a host by the address it paired with. A host whose LAN address changes must be paired again, unless the device uses a stable name (`mac.local`) or a DHCP reservation.
- Keys on mobile live in the app's private storage, not the iOS Keychain or Android Keystore.
- No QR code yet: the 20-character code is typed by hand.
- The app shows no LAN pill in the sidebar; LAN status is in Settings.
- On iOS, the first LAN connection asks for Local Network permission (`NSLocalNetworkUsageDescription` in `scripts/bundle-ios.sh`). Without it, pairing and connecting fail.
- The OS firewall may ask once whether `urushid` may accept incoming connections. That prompt is the OS's; Urushi never changes firewall rules.
- A paired device can still change other settings, including tailnet serving, as tailnet peers can (ADR 0005). Only LAN access itself is host-only.
