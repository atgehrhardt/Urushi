# 0023. Read a Jira board someone looks at every 3 s, look before reading, and show moves at once

- **Status:** Accepted
- **Date:** 2026-11-09
- **Amends:** [0020](0020-jira-boards-as-daemon-mirrors.md) (its polling intervals, and
  write-through for moves)

## Context
ADR 0020 polled every Jira board every 15 s while any client was connected (2 min
otherwise), on a 5 s tick, one board after another, and each poll read every issue
updated in the last couple of minutes in full. A move waited for Jira (about a second)
before the card settled. Users want changes made in Jira to show "near real time" and
moves to feel instant (UR-4).

Webhooks still can't reach a daemon on a laptop without a public address (ADR 0020).
Jira Cloud's rate limits count requests and their cost, so reading every board every
few seconds in full isn't an option.

## Decision
- **Clients say which board they show** (`WatchBoard{board_id}`, one per connection,
  dropped when the connection asks for another or closes). A watched board is read every
  **3 s**, others every 15 s while a client is connected and every 2 min otherwise. The
  loop ticks every second and boards sync side by side, each under its own lock.
- **Look before reading.** A sync first asks Jira only for the ids and update times of
  the board's issues updated in the last few minutes (`fields=updated`). The daemon
  remembers the times it has seen; only issues whose time is new are read in full
  (`id in (…)`). A quiet board costs one small request per look.
- **After a write, look again at once**, to catch what Jira's automation changed.
- **Moves show at once.** The card is placed where it was dropped, under the board's lock,
  and clients see it; then the transition and rank are made in Jira. If Jira refuses,
  the card goes back and the client is told why. Other edits still wait for Jira.
- A full read every 5 min still drops deleted and moved-away issues.

## Alternatives considered
- **Webhooks through Tailscale Funnel or a relay.** Instant, but needs a public
  endpoint per user and a Jira admin to register it; rejected again (and by the user).
- **Shorter intervals for every board.** Costs requests for boards nobody is looking
  at; rate limits would be reached with a few boards.
- **Optimistic edits of every field.** Field edits go through the editor's Save and
  aren't watched card by card; moves are what feels slow on a board.

## Consequences
- A change made in Jira shows within about 3 s on a board on screen, 15 s on others.
- One extra request type (`changed`) and per-board state in the daemon (seen update
  times, watches, whether a sync is running).
- A refused move is visible for about a second before it goes back.
- Jira counts update times in minutes for relative queries, so each look re-reads the
  ids of issues changed in the last couple of minutes (ids only).
