#!/usr/bin/env bash
# Builds Urushi.app and installs it into /Applications (or $INSTALL_DIR) without
# disturbing a running copy:
#   - the new bundle is staged next to the old one and swapped in with renames, so
#     running processes keep their (now unlinked) binaries and never crash mid-upgrade;
#   - the running daemon, and every terminal session it owns, is left alone. The new
#     daemon binary takes effect the next time the daemon starts (e.g. after a reboot).
set -euo pipefail
cd "$(dirname "$0")/.."
INSTALL_DIR="${INSTALL_DIR:-/Applications}"
./scripts/bundle-macos.sh
DEST="$INSTALL_DIR/Urushi.app"
STAGE="$INSTALL_DIR/.Urushi.app.new"
OLD="$INSTALL_DIR/.Urushi.app.old"
rm -rf "$STAGE" "$OLD"
ditto build/Urushi.app "$STAGE"
if [[ -d "$DEST" ]]; then
  mv "$DEST" "$OLD"
fi
mv "$STAGE" "$DEST"
rm -rf "$OLD"
# The staging bundle in build/ must not show up as a second app.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u build/Urushi.app 2>/dev/null || true
echo "installed $DEST"

# Report (but never restart) a running daemon, so the user knows which version serves them.
CLI="$DEST/Contents/Resources/bin/urushi"
if pgrep -qf "urushid --home $HOME/Library/Application Support/Urushi"; then
  echo "your daemon is still running (sessions kept):"
  STATUS="$(env -u URUSHI_HOME "$CLI" status 2>/dev/null || true)"
  echo "  ${STATUS%%$'\n'*}"
  echo "it switches to the new urushid the next time it starts. To switch now (closes your"
  echo "running terminals; panes come back from their restore points), use Settings ->"
  echo "Restart daemon in the app, or run:"
  echo "  \"$CLI\" daemon restart"
fi
echo "CLI: $CLI  (e.g. ln -s \"$CLI\" ~/.local/bin/urushi)"
