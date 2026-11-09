# Feature: Agent usage in the status bar

- **Status:** Shipped
- **Owners (crates):** `urushi-daemon` (`usage/`, `platform::{process_table, claude_credentials}`, `server/usage.rs`), `urushi-app` (`update/usage.rs`, `view/status_bar.rs`), `urushi-cli` (`urushi usage`)
- **Related ADRs:** [0011](../adr/0011-agent-usage-from-the-agents-own-sources.md), [0002](../adr/0002-daemon-source-of-truth.md)

## What it does
A status bar sits under the terminals. On the left it shows the thread's branch. When an
agent CLI runs in the focused pane, the right side shows the agent, its plan and each
usage window, with a meter, the percentage used and when the window resets. For example:
`Claude · Max 20x   5h ▬▬▭ 41% · resets in 2h 15m   Week ▬▬▬ 76% · resets in 3d 5h`.
The meter turns amber at 70% and red at 90%. The windows shown are whatever the plan has:
Claude's 5-hour and weekly limits (and per-model weekly limits when they apply), and
Codex's primary and secondary windows.

## How it works
1. Every 3 s the backend sends `Msg::Tick(now)` (`backend.rs::ticks`). The app asks
   `GetAgentUsage{pane_id}` for the focused pane, one request at a time
   (`update/usage.rs`). This only happens with daemons ≥ 1.5.
2. The daemon (`Service::agent_usage` → `usage::UsageMonitor::report`) looks up the
   pane's shell pid. It lists processes (`ps`, cached 2 s) and walks the shell's
   descendants for an agent command line (`usage::process::agent_under`). It recognises
   `claude …`, `/path/codex …`, `node …/codex.js` and Cursor's `…/cursor-agent/…`.
3. For a found agent, it reads usage (cached 5 min, failures 30 s; rate limits below):
   - **Claude** (`usage/claude.rs`): reads the sign-in from the keychain
     (`security find-generic-password -s "Claude Code-credentials" -w`) or from
     `~/.claude/.credentials.json`, then calls `curl … --header @- https://api.anthropic.com/api/oauth/usage`
     with the token on stdin. The plan comes from `subscriptionType` and `rateLimitTier`.
   - **Codex** (`usage/codex.rs`): the newest `rate_limits` record in
     `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`. A window that has reset since it
     was logged shows 0%.
   - **Cursor:** "Cursor doesn't report its plan usage".

   Claude's endpoint is rate-limited per sign-in, and every running Claude Code uses the
   same budget. An HTTP 429 comes back as `Failure::RateLimited` with its `Retry-After`,
   which curl prints through `--write-out` (`usage/claude.rs::answer`). `usage/cache.rs`
   then waits as long as asked, kept between 1 min and 1 h. With no header it waits
   5 min and doubles the wait on each refusal in a row, up to 1 h. While it waits, the bar
   shows the last good reading with its own `fetched_at`. If there is none, it says
   "Claude's usage service is busy; trying again in 4m".
4. The reply is `AgentUsage{agent, plan, windows[label, used_percent, resets_at_ms],
   unavailable, fetched_at_ms}`. An agent of `UNSPECIFIED` means a plain shell. The bar
   shows usage only if it is about the focused pane (`AppState::shown_usage`).

`urushi usage <pane>` prints the same report, for scripting and checks.

## Invariants
- The token never appears on a command line, in logs or on the wire. It is passed to
  curl on stdin (`usage/claude.rs::read`).
- Usage is always about the focused pane's own process tree. An agent in another pane
  doesn't count (`process::tests::the_nearest_agent_under_the_shell_is_found`).
- A failed read never raises the error banner. The bar shows the reason, or nothing
  (`update/tests/board.rs::ticks_ask_for_the_focused_panes_agent_usage_one_at_a_time`).
- A rate-limited service isn't asked again before its wait is over, however often clients
  poll (`usage::tests::a_rate_limited_service_is_left_alone_and_the_last_reading_stays`).

## Tests
- `crates/urushi-daemon/src/usage/`: parsing `ps`, agent detection, Claude credentials
  and both answer shapes, the status and `Retry-After` lines from curl, Codex records and
  reset handling, RFC 3339 times, caching and rate-limit waits (`cache.rs`, and with a
  fake `UsageSource`), and running helpers with a timeout.
- `crates/urushi-daemon/tests/boards_and_usage.rs::usage_follows_the_agent_running_in_the_pane`:
  a stand-in `claude` script in a real pane is detected, with a fake usage source.
- `crates/urushi-app/src/update/tests/board.rs`: polling and its answers.
  `snapshot.rs` has the `status` scenario.
- Manually verified against the real keychain and endpoint: Max plan, weekly window.

## Limitations and follow-ups
- Windows lists no processes yet (`platform/windows.rs::process_table`), so no usage.
- Cursor has no usage source.
- If `CLAUDE_CONFIG_DIR` is set, Claude stores its keychain item under a different
  name, so only the credentials file is found.
- An expired Claude sign-in isn't refreshed by Urushi. Claude refreshes it while it
  runs.
- Polling runs while the window is open, even when it's hidden.
