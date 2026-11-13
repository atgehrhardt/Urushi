#!/usr/bin/env python3
"""Copies your own settings and Jira sign-ins from the real home into the dev home.

Run by `scripts/dev.sh seed` (and `run --seed`) while the dev daemon is stopped. The
real home is only read. Settings that would make the dev daemon act as the real one are
left as the dev home has them: Tailscale serving (it would take the real daemon's port),
LAN access, and the worktree folder (dev threads must not create worktrees beside
yours).

    seed-dev-home.py settings <real home> <dev home>
        merges the settings and copies the Jira sign-ins (kept 0600)
    seed-dev-home.py boards <real home> <dev home>
        prints the real home's Jira boards the dev home lacks, one per line, as
        "<Jira board id> <sign-in key or site>", for `urushi jira import`
"""

import json
import os
import shutil
import sys

# Settings kept as the dev home has them.
KEEP_DEV = {"tailnet", "lan", "worktree_root"}
CREDENTIALS = "jira-credentials.json"
WORKSPACE = "workspace.json"


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def seed_settings(real, dev):
    real_settings = load(os.path.join(real, WORKSPACE)).get("settings", {})
    path = os.path.join(dev, WORKSPACE)
    workspace = load(path)
    settings = workspace.setdefault("settings", {})
    copied = sorted(k for k in real_settings if k not in KEEP_DEV)
    for key in copied:
        settings[key] = real_settings[key]
    tmp = path + ".seed"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(workspace, f, indent=2)
    os.replace(tmp, path)
    print("settings: " + ", ".join(copied))

    source = os.path.join(real, CREDENTIALS)
    if os.path.exists(source):
        target = os.path.join(dev, CREDENTIALS)
        # Created 0600 before any token is written into it.
        fd = os.open(target + ".seed", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "wb") as out, open(source, "rb") as src:
            shutil.copyfileobj(src, out)
        os.replace(target + ".seed", target)
        os.chmod(target, 0o600)
        accounts = load(target)
        accounts = accounts.get("accounts", [accounts])
        names = [f"{a.get('email')} at {a.get('site')}" for a in accounts]
        print("jira sign-ins: " + (", ".join(names) or "none"))
    else:
        print("jira sign-ins: none in the real home")


def jira_boards(home):
    path = os.path.join(home, WORKSPACE)
    if not os.path.exists(path):
        return []
    out = []
    for board in load(path).get("boards", []):
        source = board.get("source") or {}
        if source.get("type") == "jira":
            out.append(source)
    return out


def missing_boards(real, dev):
    have = {(b.get("site"), b.get("board_id")) for b in jira_boards(dev)}
    for b in jira_boards(real):
        if (b.get("site"), b.get("board_id")) not in have:
            print(b["board_id"], b.get("account") or b.get("site"))


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in ("settings", "boards"):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    command, real, dev = sys.argv[1:]
    if os.path.realpath(real) == os.path.realpath(dev):
        print("refusing: the dev home is the real home", file=sys.stderr)
        return 1
    if command == "settings":
        seed_settings(real, dev)
    else:
        missing_boards(real, dev)
    return 0


if __name__ == "__main__":
    sys.exit(main())
