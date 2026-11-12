# 0021. Make HTTPS requests through `curl --config -`, and keep the Jira sign-in in a private file

- **Status:** Accepted; its credentials file holds several sign-ins since [0051](0051-several-jira-sign-ins-keyed-by-site-and-email.md)
- **Date:** 2026-11-09

## Context
Jira boards (ADR 0020) make the daemon a real REST client: GET, POST and PUT with JSON
bodies, Basic authentication with an API token, pagination, rate limits. ADR 0011 already
runs `curl` for one GET, with the token in a header read from stdin. The daemon must also
keep the user's Jira API token between restarts, on macOS, Linux (often headless) and
Windows.

## Decision
- **HTTP:** `crate::http` (`HttpClient` trait, `CurlHttp`) runs the system's `curl` with
  `--config -`: the address, method, headers (credentials included) and JSON body all go
  in a curl config on **stdin**, quoted and escaped, so none of it appears in a process
  listing. curl isn't given `--fail`; the status and `Retry-After` come back through
  `--write-out`. `exec::run` writes stdin on its own thread so a large body can't stall the
  pipe or escape the time limit. Base64 and percent-encoding are a few lines in `http.rs`.
- **Credentials:** the sign-in (site, email, token) is kept in
  `<home>/jira-credentials.json`, mode 0600 inside the 0700 home, written atomically
  (`platform::write_private`). It is never part of the workspace document or settings,
  never logged (`Account`'s `Debug` hides the token) and never sent to a client. Clients
  send the token once, in `ConnectJira`, after which the daemon checks it with Jira before
  keeping it.

## Alternatives considered
- **An HTTP client crate (`ureq` or `reqwest` with `rustls`).** A large TLS dependency tree
  for the daemon, and `webpki-roots` is under CDLA-Permissive-2.0, outside our allowlist;
  the platform verifier adds more. curl already ships with the OS and uses its
  certificate store. Still rejected, as in ADR 0011.
- **Headers on stdin (`--header @-`) and the body in a temp file.** Works, but writes the
  body to disk and needs cleanup; a config on stdin carries everything.
- **The OS keychain** (macOS Keychain, Secret Service, Windows Credential Manager). Better
  at-rest protection on desktops, but each needs its own code path, Linux servers often
  have no keyring, and writing to the macOS keychain without exposing the secret in
  `security`'s arguments needs its interactive mode. A file readable only by the user
  matches how the daemon already protects its socket and state; a keychain can be added
  behind the same `CredentialStore` later.
- **Storing the token in settings.** Settings are sent to every client, local or remote.

## Consequences
- Any future HTTP integration reuses `HttpClient`, and tests fake it without a network.
- The daemon needs `curl` on the machine (present on macOS, Windows 10+ and nearly every
  Linux); without it, Jira says so.
- Anyone who can read the user's files can read the token, as with other CLI tools'
  credential files. Revoking it at id.atlassian.com or signing out (`DisconnectJira`)
  removes it.
- Claude's usage request (ADR 0011) still builds its own curl call; moving it onto
  `HttpClient` is a small follow-up.
