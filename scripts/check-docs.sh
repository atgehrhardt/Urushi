#!/usr/bin/env bash
# Documentation consistency checks (run by `make check-docs` and CI):
#   1. every Markdown file under docs/ is linked from docs/README.md
#   2. every relative Markdown link in AGENTS.md, README.md, docs/ and skills resolves
#   3. every crate under crates/ is mentioned in docs/architecture.md and AGENTS.md
#   4. every crate's lib.rs or main.rs starts with a //! module doc
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0
err() { echo "check-docs: $*" >&2; fail=1; }

while IFS= read -r f; do
  rel="${f#docs/}"
  [ "$rel" = "README.md" ] && continue
  grep -qF "($rel)" docs/README.md || err "docs/$rel is not listed in docs/README.md"
done < <(find docs -name '*.md' | sort)

while IFS= read -r f; do
  dir="$(dirname "$f")"
  # Extract link targets: ](target) — skip URLs and in-page anchors.
  grep -oE '\]\([^)#]+' "$f" | sed 's/^](//' | while IFS= read -r target; do
    case "$target" in http*|mailto:*|*NNNN*|*…*) continue ;; esac  # URLs and template placeholders
    [ -e "$dir/$target" ] || { echo "check-docs: broken link in $f -> $target" >&2; echo x; }
  done | grep -q x && fail=1 || true
done < <(find AGENTS.md README.md docs .claude/skills -name '*.md' | sort)

for crate in crates/*/; do
  name="$(basename "$crate")"
  grep -q "$name" docs/architecture.md || err "crate $name is not mentioned in docs/architecture.md"
  grep -q "$name" AGENTS.md || err "crate $name is not in the AGENTS.md crate map"
  root="$crate/src/lib.rs"; [ -f "$root" ] || root="$crate/src/main.rs"
  [ -f "$root" ] && { head -1 "$root" | grep -q '^//!' || err "$root must start with a //! module doc"; }
done

[ "$fail" -eq 0 ] && echo "check-docs: ok"
exit "$fail"
