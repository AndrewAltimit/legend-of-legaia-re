#!/usr/bin/env bash
# state-walk.sh - extract overlay slices from EVERY scenario in scenarios.toml.
#
# For each scenario in the manifest, slices the configured overlay window
# out of main RAM, writes it to /tmp/legaia_overlay_<label>.bin, and
# (optionally) imports it as a labelled program in the Ghidra container.
#
# Usage:
#   scripts/mednafen/state-walk.sh                # extract only (no Ghidra)
#   scripts/mednafen/state-walk.sh --import       # extract + import per label
#   scripts/mednafen/state-walk.sh --only mc4     # restrict to one slot
#
# Pre-reqs:
#   - cargo build --release  (provides target/release/mednafen-state)
#   - extracted/SCUS_942.54  (anchor strings)
#   - $HOME/.mednafen/mcs/ contains the saves named in scenarios.toml
#     (or set LEGAIA_MEDNAFEN_DIR to point elsewhere)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

MANIFEST="scripts/scenarios.toml"
DO_IMPORT=0
ONLY_SLOT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --import) DO_IMPORT=1; shift ;;
        --only) ONLY_SLOT="$2"; shift 2 ;;
        --manifest) MANIFEST="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,16p' "$0"
            exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 64 ;;
    esac
done

if [[ ! -f "$MANIFEST" ]]; then
    echo "manifest not found: $MANIFEST" >&2
    exit 66
fi

CLI="target/release/mednafen-state"
if [[ ! -x "$CLI" ]]; then
    echo "[info] building mednafen-state..."
    cargo build --release -p legaia-mednafen >/dev/null
fi

# Use the CLI's `scenarios` subcommand to list scenarios in a parseable form.
# Each line is "mc<slot>  <label>  <description>...".
mapfile -t LINES < <("$CLI" scenarios --manifest "$MANIFEST" 2>/dev/null \
    | awk '/^  mc[0-9]/ { print }')

for line in "${LINES[@]}"; do
    # parse "  mc<slot>  <label>  <description>"
    SLOT=$(echo "$line" | awk '{print $1}' | sed 's/mc//')
    LABEL=$(echo "$line" | awk '{print $2}')
    if [[ -n "$ONLY_SLOT" && "$ONLY_SLOT" != "mc$SLOT" && "$ONLY_SLOT" != "$SLOT" ]]; then
        continue
    fi

    SAVE_DIR="${LEGAIA_MEDNAFEN_DIR:-$HOME/.mednafen/mcs}"
    SAVE="$SAVE_DIR/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc$SLOT"
    OUT="/tmp/legaia_overlay_${LABEL}.bin"

    if [[ ! -f "$SAVE" ]]; then
        echo "[skip] mc$SLOT ($LABEL): $SAVE not found"
        continue
    fi

    echo "[extract] mc$SLOT -> $OUT"
    "$CLI" extract "$SAVE" --out "$OUT" >/dev/null

    if [[ "$DO_IMPORT" == "1" ]]; then
        echo "[import] $LABEL"
        scripts/import-overlay-named.sh "$OUT" "$LABEL" >/dev/null 2>&1 \
            || echo "[warn]  import failed for $LABEL"
    fi
done

echo "[ok] walked $(echo "${LINES[@]}" | wc -w | awk '{print $1/3}') scenarios"
