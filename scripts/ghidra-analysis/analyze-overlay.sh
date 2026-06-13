#!/usr/bin/env bash
# End-to-end pipeline: mednafen save state -> overlay extract ->
# Ghidra import + analysis -> asset-loader call CSV.
#
# Usage:
#   scripts/ghidra-analysis/analyze-overlay.sh ~/.mednafen/mcs/Legend\ of\ Legaia\ \(USA\).<HASH>.mc0 \
#       [--label level_up]
#
# Output:
#   /tmp/legaia_overlay_<label>.bin  (raw 192 KB overlay slice)
#   /tmp/overlay_loads_<label>.csv   (loader-call CSV)
#   stdout: human-readable summary of asset-loader calls found
#
# Pre-reqs:
#   - docker compose stack with ghidra service running
#   - extracted/SCUS_942.54 (for the anchor strings)

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <save-state.mc0> [--label NAME]" >&2
    exit 64
fi

SAVE="$1"
shift
LABEL="capture_$(date +%H%M%S)"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --label) LABEL="$2"; shift 2 ;;
        *) echo "unknown arg: $1" >&2; exit 64 ;;
    esac
done

if [[ ! -f "$SAVE" ]]; then
    echo "save state not found: $SAVE" >&2
    exit 66
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DUMP="/tmp/legaia_overlay_${LABEL}.bin"
CSV="/tmp/overlay_loads_${LABEL}.csv"

cd "$REPO_ROOT"

echo ">>> 1/3 Extracting overlay slice from save state..."
scripts/ghidra-analysis/extract-mednafen-overlay.py "$SAVE" --out "$DUMP"

echo
echo ">>> 2/3 Importing into Ghidra (this re-imports as overlay.bin)..."
ghidra/scripts/import_overlay.sh "$DUMP" 2>&1 | tail -5

echo
echo ">>> 3/3 Scanning overlay for asset-loader calls..."
docker compose exec -T ghidra /ghidra/support/analyzeHeadless /projects legaia \
    -process overlay.bin -noanalysis \
    -postScript /scripts/find_overlay_asset_loads.py 2>&1 \
    | grep -E "^(loader|FUN_)" > "$CSV"

echo
echo "=== Asset-loader calls in $LABEL overlay ==="
column -ts, "$CSV" | head -50
echo
echo "Total loader calls: $(($(wc -l < "$CSV") - 1))"
echo
echo "Outputs:"
echo "  $DUMP"
echo "  $CSV"
echo
echo "Next: take any 'index' arg whose arg_value is a hex constant and"
echo "look up the CDNAME block in extracted/CDNAME.TXT to identify the"
echo "PROT entry the overlay loads. Add those to --bundle in the viewer."
