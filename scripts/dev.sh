#!/usr/bin/env bash
# Safe development loop for working on Urushi *from inside* Urushi.
#
# Your real workspace (the installed app, and the daemon in the real home that owns your
# terminals) is never stopped, signalled or connected to. Everything here uses the dev home.
#
#   scripts/dev.sh run [--keep] [--seed]
#                                 build debug binaries, restart the *dev* daemon (unless
#                                 --keep), and open the dev app ("Urushi (dev)", DEV badge);
#                                 --seed runs `seed` first
#   scripts/dev.sh seed           give the dev home your settings, Jira sign-ins and Jira
#                                 boards (read from the real home, which is never written)
#   scripts/dev.sh test           make ci, with URUSHI_HOME pointed at a throwaway dir
#   scripts/dev.sh cli <args>     the debug CLI against the dev daemon, e.g. `cli ls`
#   scripts/dev.sh status         show the dev and real daemons (read-only)
#   scripts/dev.sh stop           stop the dev daemon (only it, verified by pidfile and home)
#   scripts/dev.sh ship           make ci, then install into /Applications (never restarts
#                                 your running daemon; relaunch the app to use the new GUI)
#
# The dev home starts empty. `seed` copies your settings (but not Tailscale serving, LAN
# access or the worktree folder, which stay the dev home's) and Jira sign-ins, then
# imports each Jira board of yours the dev home lacks. Run it again any time: it only
# adds. The dev daemon then syncs your real Jira boards: moves and edits made in the
# dev app are made in Jira.
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)" in
  Darwin) BASE="$HOME/Library/Application Support"; REAL_HOME="$BASE/Urushi"; DEFAULT_DEV="$BASE/Urushi Dev" ;;
  *) BASE="${XDG_DATA_HOME:-$HOME/.local/share}"; REAL_HOME="$BASE/urushi"; DEFAULT_DEV="$BASE/urushi-dev" ;;
esac
DEV_HOME="${URUSHI_DEV_HOME:-$DEFAULT_DEV}"
if [[ "$(cd "$(dirname "$DEV_HOME")" 2>/dev/null && pwd)/$(basename "$DEV_HOME")" == "$REAL_HOME" ]]; then
  echo "refusing: the dev home must not be the real home ($REAL_HOME)" >&2
  exit 1
fi
export URUSHI_HOME="$DEV_HOME"
CARGO="${CARGO:-cargo}"

# Stops the dev daemon. Only a PID from the dev home's pidfile, and only if that process
# really is a urushid serving the dev home.
stop_dev_daemon() {
  local pidfile="$DEV_HOME/urushid.pid"
  [[ -f "$pidfile" ]] || return 0
  local pid; pid="$(tr -dc '0-9' < "$pidfile")"
  [[ -n "$pid" ]] || return 0
  local cmd; cmd="$(ps -o command= -p "$pid" 2>/dev/null || true)"
  if [[ "$cmd" == *urushid* && "$cmd" == *"--home $DEV_HOME"* ]]; then
    kill "$pid"
    for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    echo "stopped dev daemon ($pid)"
  fi
  rm -f "$pidfile"
}

build() {
  "$CARGO" build -p urushi-desktop -p urushi-daemon -p urushi-cli
}

# Copies settings and Jira sign-ins from the real home (read only) into the dev home,
# then imports the real home's Jira boards the dev home lacks.
seed() {
  [[ -d "$REAL_HOME" ]] || { echo "no real home at $REAL_HOME: nothing to seed" >&2; return 1; }
  "$CARGO" build -q -p urushi-cli -p urushi-daemon
  local cli=(./target/debug/urushi --home "$DEV_HOME")
  # The dev daemon writes its workspace when it starts: make sure there is one.
  "${cli[@]}" ls >/dev/null
  stop_dev_daemon
  python3 -I scripts/seed-dev-home.py settings "$REAL_HOME" "$DEV_HOME"
  local board account
  while read -r board account; do
    [[ -n "$board" ]] || continue
    echo "importing Jira board $board ($account)"
    "${cli[@]}" jira import "$board" --account "$account" >/dev/null \
      || echo "  couldn't import Jira board $board" >&2
  done < <(python3 -I scripts/seed-dev-home.py boards "$REAL_HOME" "$DEV_HOME")
}

case "${1:-}" in
  run)
    shift
    keep=false
    for arg in "$@"; do
      case "$arg" in
        --keep) keep=true ;;
        --seed) seed ;;
        *) echo "unknown option: $arg" >&2; exit 1 ;;
      esac
    done
    build
    $keep || stop_dev_daemon
    echo "dev home: $DEV_HOME"
    exec ./target/debug/urushi-app --home "$DEV_HOME"
    ;;
  test)
    # Tests already use temp homes. This makes doubly sure nothing can reach a real one.
    URUSHI_HOME="$(mktemp -d "${TMPDIR:-/tmp}/urushi-test.XXXXXX")"
    export URUSHI_HOME
    make ci
    ;;
  cli)
    shift
    "$CARGO" build -q -p urushi-cli -p urushi-daemon
    exec ./target/debug/urushi --home "$DEV_HOME" "$@"
    ;;
  status)
    echo "dev home:  $DEV_HOME"
    if [[ -f "$DEV_HOME/urushid.pid" ]]; then echo "dev daemon: pid $(cat "$DEV_HOME/urushid.pid")"; else echo "dev daemon: not running"; fi
    echo "real home: $REAL_HOME (left alone)"
    if [[ -f "$REAL_HOME/urushid.pid" ]]; then echo "real daemon: pid $(cat "$REAL_HOME/urushid.pid")"; else echo "real daemon: running without pidfile, or not running"; fi
    ;;
  stop)
    stop_dev_daemon
    ;;
  seed)
    seed
    ;;
  ship)
    "$0" test
    ./scripts/install-macos.sh
    echo "Quit and reopen Urushi to use the new app. Your daemon and terminals keep running."
    ;;
  *)
    sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
