#!/usr/bin/env bash
# Bulk-import every PROT entry that find-overlay surfaces as MIPS code into
# the Ghidra project, then run the inventory dumper across all of them.
#
# Default base address is 0x801C0000 (the overlay window). Specific overlays
# may load at higher offsets (e.g. 0897 at 0x801CE818); for survey purposes
# the default base is fine - function discovery and string xrefs work, only
# specific RAM-address xrefs need the exact base.
#
# Usage:
#   scripts/ghidra-analysis/bulk-import-overlays.sh [--score 3.0] [--limit 30] [--base 0x801C0000]
#
# Output:
#   - one Ghidra program per imported PROT entry (named after the file stem)
#   - per-program inventory CSVs at ghidra/scripts/inventory_<stem>.csv
#   - skips entries that are already in the project

set -euo pipefail

SCORE=3.0
LIMIT=30
BASE="0x801C0000"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --score) SCORE="$2"; shift 2 ;;
        --limit) LIMIT="$2"; shift 2 ;;
        --base)  BASE="$2"; shift 2 ;;
        -h|--help)
            sed -n '1,/^set -euo pipefail/p' "$0" | sed 's/^# *//;s/^#//' >&2
            exit 0
            ;;
        *) echo "unknown arg: $1" >&2; exit 64 ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

if ! docker compose ps ghidra | grep -q Up; then
    echo "ghidra container isn't running; bring it up first:" >&2
    echo "  docker compose up -d ghidra" >&2
    exit 1
fi

# Capture find-overlay output, filter by score, take top LIMIT.
# Output columns: rank size mode out_size jr_ra prol score path
mapfile -t CANDIDATES < <(
    ./target/release/asset find-overlay extracted/PROT --top "$LIMIT" \
        | awk -v s="$SCORE" 'NR>1 && $7+0 >= s+0 { print $8 }'
)

if [[ ${#CANDIDATES[@]} -eq 0 ]]; then
    echo "no candidates with score >= $SCORE in top $LIMIT"
    exit 0
fi

echo "Bulk-importing ${#CANDIDATES[@]} overlay candidates at base $BASE"
echo

# Get the existing-program list so we can skip already-imported ones.
EXISTING=$(docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -noanalysis -preScript /scripts/list_programs.py 2>&1 \
    | grep -E "^[0-9a-zA-Z_.]+\s+\[Program\]" | awk '{print $1}')

for CANDIDATE in "${CANDIDATES[@]}"; do
    BASENAME="$(basename "$CANDIDATE")"
    STEM="${BASENAME%.BIN}"
    PROG_NAME="overlay_${STEM}.bin"

    if echo "$EXISTING" | grep -Fxq "$PROG_NAME"; then
        echo "[skip] $PROG_NAME already imported"
        continue
    fi

    SRC="extracted/PROT/$BASENAME"
    if [[ ! -f "$SRC" ]]; then
        echo "[skip] $SRC not found"
        continue
    fi

    echo "[import] $BASENAME -> $PROG_NAME @ $BASE"
    docker compose cp "$SRC" "ghidra:/tmp/$PROG_NAME"
    docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
        /projects legaia \
        -import "/tmp/$PROG_NAME" \
        -loader BinaryLoader \
        -loader-baseAddr "$BASE" \
        -processor MIPS:LE:32:default \
        -overwrite 2>&1 | tail -3

    echo "[analyze] $PROG_NAME"
    docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
        /projects legaia -process "$PROG_NAME" 2>&1 | tail -2

    echo "[inventory] $PROG_NAME"
    docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
        /projects legaia -process "$PROG_NAME" -noanalysis \
        -postScript /scripts/inventory_overlay.py 2>&1 | grep -E "(wrote|ERROR)"

    echo
done

# Normalize ownership of CSVs that landed.
docker compose exec -T ghidra chown -R "$(id -u):$(id -g)" /scripts 2>/dev/null || true

echo "Done. Inventory CSVs in ghidra/scripts/inventory_*.csv"
