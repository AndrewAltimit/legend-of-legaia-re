#!/usr/bin/env bash
# auto-capture.sh - for every scenario, run all configured watchpoints
# against its `diff_against` sister states and write per-scenario JSON
# reports to /tmp/legaia_watch_<label>.json.
#
# This is the "watchpoint-equivalent" path: instead of setting a real
# memory breakpoint in mednafen, we diff before/after main RAM and
# surface every region whose contents changed in the configured window.
#
# Usage:
#   scripts/mednafen/auto-capture.sh                  # all scenarios
#   scripts/mednafen/auto-capture.sh --label area_load_early
#
# Outputs:
#   /tmp/legaia_watch_<label>.json    # JSON per-watchpoint diff results
#   stdout                            # human summary
#
# See `docs/tooling/mednafen-automation.md` for how to read the output.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

MANIFEST="scripts/scenarios.toml"
ONE_LABEL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --label) ONE_LABEL="$2"; shift 2 ;;
        --manifest) MANIFEST="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,18p' "$0"
            exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 64 ;;
    esac
done

CLI="target/release/mednafen-state"
if [[ ! -x "$CLI" ]]; then
    echo "[info] building mednafen-state..."
    cargo build --release -p legaia-mednafen >/dev/null
fi

mapfile -t LABELS < <("$CLI" scenarios --manifest "$MANIFEST" 2>/dev/null \
    | awk '/^  mc[0-9]/ { print $2 }')

for LABEL in "${LABELS[@]}"; do
    if [[ -n "$ONE_LABEL" && "$ONE_LABEL" != "$LABEL" ]]; then
        continue
    fi
    OUT="/tmp/legaia_watch_${LABEL}.json"
    echo "==== $LABEL ===="
    if "$CLI" watch "$LABEL" --manifest "$MANIFEST" --json "$OUT" 2>&1 | sed 's/^/    /'; then
        echo "    -> $OUT"
    else
        echo "    [warn] watch failed for $LABEL"
    fi
done
