# 0011. Read agent plan usage in the daemon, from each agent's own sign-in and logs

- **Status:** Proposed
- **Date:** 2026-11-07

## Context
The status bar shows the plan usage of the agent CLI running in the focused pane, e.g.
Claude's 5-hour and weekly limits. None of the agents offers a documented API for this.
Each one keeps it differently:
- **Claude Code** asks `GET https://api.anthropic.com/api/oauth/usage` with its OAuth
  sign-in. Its `/usage` screen shows the answer. The sign-in is in the macOS keychain item
  "Claude Code-credentials", which Claude reads and writes with `security`, or in
  `~/.claude/.credentials.json`.
- **Codex** logs `rate_limits` (primary and secondary windows, plan type) in its session
  files under `~/.codex/sessions` after every reply.
- **Cursor** exposes nothing we can find.

Agents run on the daemon's machine. A phone or a remote client has neither the agent
nor its sign-in. We also need to know *which* agent is running in a pane. The user may
start `claude` by hand in a terminal tab, so the tab's launch choice isn't enough.

## Decision
- The **daemon** answers `GetAgentUsage{pane_id}` (protocol 1.5) with
  `urushi-daemon::usage::UsageMonitor`.
- **Which agent:** the first descendant of the pane's shell whose command line is an
  agent CLI (`usage::process`). It is read from `ps -A -o pid=,ppid=,args=`, through
  `platform::process_table`. Windows lists nothing for now.
- **Claude:** read the sign-in the way Claude does (`platform::claude_credentials`). Then
  call the usage endpoint with **`curl`**, and pass the token on **stdin**
  (`--header @-`) so it never appears in a process listing. Parsing is lenient. It
  understands both the newer `limits` list and the older `five_hour` / `seven_day`
  objects, and an unexpected answer is shown as "usage unavailable" rather than an
  error.
- **Codex:** read the newest `rate_limits` record from the most recent session files.
- **Caching:** the process table for 2 s, usage for 5 min, failures for 30 s. Clients
  poll every 3 s. Each read is behind the `UsageSource` trait, so tests never touch the
  real keychain, network or home directory.
- **Rate limits:** Claude's endpoint is rate-limited per sign-in, and every running Claude
  Code uses the same budget. After an HTTP 429 the daemon waits as long as `Retry-After`
  says, kept between 1 min and 1 h. With no header it waits 5 min and doubles the wait on
  each refusal in a row, up to 1 h, like Claude Code does. While it waits, the bar shows
  the last good reading (`usage/cache.rs`). Retrying every 30 s kept the budget used up,
  so the bar never recovered.

## Alternatives considered
- **Read usage in the client:** this is the wrong machine for remote and mobile clients.
  It would also put agent knowledge in clients, against ADR 0002.
- **An HTTP client crate (reqwest + rustls):** this adds a large TLS dependency tree to
  the daemon for a single GET. The daemon already relies on external programs (`git`,
  `tailscale`), and `curl` ships with macOS, Windows 10+ and nearly every Linux.
- **Parse the agent's own screen (`/usage` output):** this would mean typing into the
  user's session. It is fragile and visible.
- **Claude Code's status-line hook:** it receives rate limits, but using it means
  rewriting the user's Claude settings, and it only runs while Claude redraws.
- **Record the agent on the tab when the launcher starts it:** this misses agents typed
  by hand and agents that have exited.

## Consequences
- Usage appears for agents started any way, within one poll of starting them. The
  numbers can be up to 5 minutes old (longer while Claude's service is refusing). Reset
  countdowns stay current because clients compute them from `resets_at`.
- The Claude endpoint is not a published API. If it changes, the bar says "usage
  unavailable" until `usage/claude.rs` is updated, and nothing else breaks.
- The daemon reads the user's Claude sign-in. It is used only for this request, never
  stored, logged or sent to clients.
- A Claude sign-in that has expired isn't refreshed by Urushi. Claude refreshes it while
  it runs, which is exactly when usage is shown.
- Windows shows no usage until it gets a process listing.
