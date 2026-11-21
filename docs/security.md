# Security

Who Urushi trusts, what protects each way in, and what the last audit found. Keep this
current: a change that adds a way in, a secret, a subprocess or a file with private
contents updates the matching row here.

Last full audit: UR-57, 2026-11-20 (code, CI, apps and dependencies at 0.1.8).

## Threat model
A Urushi connection is a shell on the host: a client can type into terminals, so any
client the daemon admits can do whatever the user can. Security is therefore about who
gets admitted, and about keeping what others write (Jira, web pages, programs in a pane)
from acting as the user.

| Who | Trusted with | How it's admitted |
|---|---|---|
| The same OS user on the host (the app, the CLI, **any process in a pane**, agents included) | Everything | The local socket is in the 0700 home, is 0600 itself, and checks the peer's uid (`platform/unix.rs::serve_local`). On Windows it is a named pipe that refuses remote clients. |
| An allowed tailnet login | Everything except stopping the daemon, updating the app, and changing who may connect (ADRs 0005, 0041, 0061) | `tailscale whois` before the handshake, checked by `server/remote.rs::authorize`. It is checked again whenever the tailnet settings change. Off by default. |
| A paired LAN device | The same as a tailnet login | Noise IK with a key paired through a one-time 100-bit code (ADR 0041). It must come from a private address. Off by default. |
| Jira, GitHub, web pages, files `cat`ed in a pane | Nothing: their text is data | Every row below. |

Not defended against: someone who already runs code as the user, and a compromised
GitHub account that publishes a release (see *Accepted risks*).

## What protects each way in
| Surface | Protection | Where |
|---|---|---|
| Local socket | 0700 home, 0600 socket, uid check | `crates/urushi-daemon/src/platform/unix.rs` |
| Tailnet listener | Binds only the tailnet addresses. Every connection goes through `whois` and `authorize`. Revoked access ends live sessions. At most 8 identifications pending. | `server/remote.rs` |
| LAN listener | Private addresses only, Noise, pairing codes with a TTL and a failure limit, at most 8 handshakes, revocation ends sessions | `server/lan.rs`, `lan/`, `urushi_proto::lan` |
| Host-only requests | Stopping the daemon, `InstallUpdate`, all LAN and tailnet access changes | `server/handlers.rs::host_only` |
| Frames | 8 MiB cap, checked before reading. The buffer grows only with the bytes that arrive. | `urushi_proto::codec` |
| Requests per connection | At most 512 in flight, then `UNAVAILABLE` | `server/connection.rs::MAX_REQUESTS_IN_FLIGHT` |
| Pasted text | Every control character except tab and CR is dropped, so nothing leaves bracketed paste | `urushi_mux::keys::encode_paste` |
| Terminal output | OSC strings are capped at 4 KiB (the title stack can't grow huge). Clipboard (OSC 52) and hyperlinks are never acted on. Query replies hold no output text. | `emulator/osc_limit.rs`, `emulator/alacritty.rs` |
| Git | Branch names, prefixes and issue keys are validated types. `--end-of-options` or `--` where values come from outside. Worktree paths are slugs. | `urushi_core::naming`, `git/cli.rs` |
| HTTPS | curl config on stdin, every value quoted. `proto`/`proto-redir` set to `=https`. Jira sites must be https. Attachment URLs must be on the account's site. | `http.rs`, `jira/credentials.rs` |
| Secrets at rest | Jira tokens, the LAN key, the workspace, restore points and attachments are 0600 in the 0700 home (`write_private`). Android keeps its data out of backups. | `platform/*::write_private`, `apps/android/AndroidManifest.xml` |
| Agents and Jira text | Task text reaches an agent only through the environment, never the typed command line. A prompt can't start with `-`. The CLI prints no control characters. | `urushi_core::agent`, `board/message.rs`, `urushi-cli/src/printable.rs` |
| Opening things | URLs must be `https://`. Attachments open from `<home>/opened-attachments` (0700), quarantined on macOS. | `urushi-app/src/platform/desktop.rs`, `files.rs` |
| Updates | Only from the daemon's own bundle and the host, after confirming. The feed is GitHub's API. Files must be under Urushi's own release downloads. SHA-256, bundle id, version and `codesign --verify --strict` are checked. | `update/` (ADR 0049) |
| Client fallback | Without a LAN pairing, the plain protocol goes only to Tailscale addresses or loopback | `urushi_client::lan::tailnet_endpoint`, `urushi_proto::endpoint::plain_tcp_allowed` |
| CI | Tokens read-only except the release's publish job. Third-party actions pinned to commits. The keystore is deleted right after signing. cargo-deny (licenses, advisories, sources) on every push and weekly. | `.github/workflows/` |

## UR-57 findings
Severity is for this threat model. *Fixed* means fixed in the UR-57 branch, with a test.

| # | Finding | Severity | Status |
|---|---|---|---|
| 1 | A nested end marker (`\e[20\e[201~1~`) broke out of bracketed paste, so a pasted clipboard or a drafted Jira message ran as typed commands | High | Fixed (`encode_paste`) |
| 2 | A remote client could change tailnet serving, port, allowlist and tagged nodes, so a paired phone could open the daemon to anyone | Medium | Fixed (ADR 0061) |
| 3 | Tailnet sessions survived turning serving off or removing a login | Medium | Fixed (ADR 0061) |
| 4 | A long title plus `CSI 22 t` pushes made the daemon allocate gigabytes, and unterminated OSC payloads grew without limit | Medium | Fixed (`OscLimit`) |
| 5 | Requests in flight per connection were unbounded, so a client that never reads could exhaust memory | Medium | Fixed (cap 512) |
| 6 | The Android app's LAN private key was included in Google backup and device transfer | Medium | Fixed (backups off) |
| 7 | Release secrets are readable by any workflow run by someone with write access (no protected environment) | Medium | Deferred: GitHub settings (sub-task) |
| 8 | Updates prove the zip is intact, not who published it (ad-hoc signature, checksum from the same release) | Medium | Hardened (URL pinning, HTTPS-only). Signing deferred (sub-task). |
| 9 | Tailnet `whois` identifications were unbounded (one process and one thread per connection attempt) | Low | Fixed |
| 10 | A remote host not in the keyring got the plain protocol whatever it resolved to | Low | Fixed (`plain_tcp_allowed`) |
| 11 | curl followed redirects with no protocol limit | Low | Fixed (`proto-redir = =https`) |
| 12 | Opened attachments went to a shared temp folder unquarantined, so an attached `.command` ran without Gatekeeper asking | Low | Fixed |
| 13 | Upload staging reused any folder already at its temp path | Low | Fixed (`create_private_dir`) |
| 14 | The workspace, restore points (scrollback, argv) and attachments were written 0644 | Low | Fixed (0600) |
| 15 | A Jira summary could become an agent option through a custom message starting `{title}`. The issue key was unquoted in the `board show` command. `board show` printed escape sequences. | Low | Fixed |
| 16 | `shell::quote` left `\` and a leading `=` unquoted | Low | Fixed |
| 17 | One frame header made the reader allocate 8 MiB up front | Low | Fixed |
| 18 | Actions pinned by tag, default token permissions, no scheduled advisory check, keystore left on the runner, signing files not in `.gitignore` | Low | Fixed |
| 19 | Windows: the named pipe and private files rely on default and inherited ACLs. A local user could squat the pipe name. | Medium on shared Windows machines; no Windows releases yet | Deferred (sub-task) |
| 20 | Mobile clients keep the LAN private key as plaintext JSON, not in the Keychain or Keystore | Low | Deferred (sub-task) |

## Accepted risks
- **Processes in a pane have full control.** They run as the user and can reach the
  local socket (agents are told the CLI's path on purpose). Urushi is not a sandbox.
- **Anyone who can publish a GitHub release can ship an update.** Users confirm each
  update, but nothing proves the publisher until releases are Developer ID signed and
  notarized (*Follow-ups*).
- **Hooks are spoofable.** Any output in a pane can print Urushi's `OSC 777;urushi;…`
  signals. They change only status badges and when a drafted message is pasted.
- **`allow-tagged-nodes` admits every tagged device** in the tailnet, whatever the
  allowlist says. It is off by default; tag ACLs in Tailscale are the place to narrow it.
- **LAN connect messages can be replayed.** Noise IK's first message has no replay
  protection. A replay only marks the device seen; without the keys the connection dies
  at the Hello timeout.
- **A restored pane re-types the command it was running** (ADR 0013), as the user left
  it.
- **The user's git config and hooks run** with git (ADR 0004), as they would in a
  terminal.

## Follow-ups
- Sign macOS releases with a Developer ID and notarize them, then pin the team in the
  updater's `codesign -R` check.
- Move the Android release secrets to a protected `release` environment with required
  reviewers, and add `environment: release` to the jobs that use them.
- Windows: an owner-only security descriptor on the named pipe, a client token check,
  and owner-only ACLs in `write_private`/`secure_dir`.
- Mobile: keep the LAN key in the iOS Keychain or Android Keystore.
