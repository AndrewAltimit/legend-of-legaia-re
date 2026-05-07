#!/usr/bin/env bash
# sweep-overlays.sh — Batch process mednafen save states into named Ghidra
# programs + per-overlay function inventory CSVs.
#
# Usage:
#   scripts/sweep-overlays.sh <spec-file>
#   scripts/sweep-overlays.sh --state <path> --label <name> [--state ... --label ...]
#
# Spec-file format (one entry per line, whitespace-separated; # = comment):
#   <save-state-path>  <label>
#   ~/.mednafen/mcs/Legend_of_Legaia.mc0  world_map
#
# Each entry:
#   1. Extracts the 192 KB overlay window -> /tmp/legaia_overlay_<label>.bin
#   2. Imports it as overlay_<label>.bin in the Ghidra project (base 0x801C0000)
#   3. Runs inventory_overlay.py -> ghidra/scripts/inventory_overlay_<label>.bin.csv
#
# Options:
#   --no-extract    Skip step 1 (use existing /tmp/legaia_overlay_<label>.bin)
#   --no-import     Skip step 2 (assume program already in Ghidra project)
#   --dry-run       Print commands without running them
#   --base ADDR     Overlay load address (default: 0x801C0000)
#   -h / --help     Show this message

set -euo pipefail

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
DRY_RUN=0
NO_EXTRACT=0
NO_IMPORT=0
BASE="0x801C0000"
declare -a STATES=()
declare -a LABELS=()
PENDING_STATE=""

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
usage() {
    sed -n '1,/^set -euo/p' "$0" | sed 's/^# *//;/^$/d;s/^#!//' >&2
    exit "${1:-0}"
}

if [[ $# -eq 0 ]]; then
    usage 64
fi

while [[ $# -gt 0 ]]; do
    case "$1" in
        --state)
            [[ -n "$PENDING_STATE" ]] && { echo "ERROR: --state without --label before it" >&2; exit 64; }
            PENDING_STATE="$2"; shift 2 ;;
        --label)
            [[ -z "$PENDING_STATE" ]] && { echo "ERROR: --label without preceding --state" >&2; exit 64; }
            STATES+=("$PENDING_STATE")
            LABELS+=("$2")
            PENDING_STATE=""; shift 2 ;;
        --no-extract)  NO_EXTRACT=1; shift ;;
        --no-import)   NO_IMPORT=1; shift ;;
        --dry-run)     DRY_RUN=1; shift ;;
        --base)        BASE="$2"; shift 2 ;;
        -h|--help)     usage 0 ;;
        -*)
            echo "ERROR: unknown option $1" >&2; usage 64 ;;
        *)
            # Positional arg: treat as spec file
            SPEC="$1"; shift
            if [[ ! -f "$SPEC" ]]; then
                echo "ERROR: spec file not found: $SPEC" >&2; exit 66
            fi
            while IFS= read -r line; do
                # Strip comments and blank lines
                line="${line%%#*}"
                line="${line#"${line%%[![:space:]]*}"}"  # ltrim
                [[ -z "$line" ]] && continue
                read -r s l <<< "$line"
                [[ -z "$s" || -z "$l" ]] && { echo "WARN: malformed spec line, skipping: $line" >&2; continue; }
                STATES+=("$s")
                LABELS+=("$l")
            done < "$SPEC"
            ;;
    esac
done

[[ -n "$PENDING_STATE" ]] && { echo "ERROR: dangling --state with no --label" >&2; exit 64; }

if [[ ${#STATES[@]} -eq 0 ]]; then
    echo "ERROR: no (state, label) pairs provided" >&2
    usage 64
fi

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

run() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "[dry-run] $*"
    else
        "$@"
    fi
}

check_ghidra() {
    if [[ "$DRY_RUN" -eq 1 ]]; then return; fi
    if ! docker compose ps ghidra 2>/dev/null | grep -q Up; then
        echo "ERROR: Ghidra container is not running." >&2
        echo "  Start it with: docker compose up -d ghidra" >&2
        exit 1
    fi
}

# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------
check_ghidra

TOTAL=${#STATES[@]}
PASS=0
FAIL=0

echo "Sweeping $TOTAL overlay(s) [base=$BASE]"
echo

for i in "${!STATES[@]}"; do
    SAVE="${STATES[$i]}"
    LABEL="${LABELS[$i]}"
    DUMP="/tmp/legaia_overlay_${LABEL}.bin"
    PROG_NAME="overlay_${LABEL}.bin"
    IDX=$((i + 1))

    echo "=== [$IDX/$TOTAL] $LABEL ==="

    # Expand ~ in save path
    SAVE_EXPANDED="${SAVE/#\~/$HOME}"

    # Step 1: extract overlay slice from save state
    if [[ "$NO_EXTRACT" -eq 1 ]]; then
        echo "  [1/3] extract: skipped (--no-extract)"
        if [[ ! -f "$DUMP" && "$DRY_RUN" -eq 0 ]]; then
            echo "  ERROR: $DUMP not found and --no-extract requested" >&2
            FAIL=$((FAIL + 1)); continue
        fi
    else
        if [[ ! -f "$SAVE_EXPANDED" && "$DRY_RUN" -eq 0 ]]; then
            echo "  ERROR: save state not found: $SAVE_EXPANDED" >&2
            FAIL=$((FAIL + 1)); continue
        fi
        echo "  [1/3] extract: $SAVE_EXPANDED -> $DUMP"
        if ! run scripts/extract-mednafen-overlay.py "$SAVE_EXPANDED" --out "$DUMP"; then
            echo "  ERROR: extraction failed for $LABEL" >&2
            FAIL=$((FAIL + 1)); continue
        fi
    fi

    # Step 2: import into Ghidra as a named program
    if [[ "$NO_IMPORT" -eq 1 ]]; then
        echo "  [2/3] import: skipped (--no-import)"
    else
        echo "  [2/3] import: $DUMP -> $PROG_NAME @ $BASE"
        if [[ "$DRY_RUN" -eq 0 ]]; then
            docker compose cp "$DUMP" "ghidra:/tmp/${PROG_NAME}"
            docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
                /projects legaia \
                -import "/tmp/${PROG_NAME}" \
                -loader BinaryLoader \
                -loader-baseAddr "$BASE" \
                -processor MIPS:LE:32:default \
                -overwrite 2>&1 | tail -3
            docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
                /projects legaia -process "${PROG_NAME}" 2>&1 | tail -2
        else
            run docker compose cp "$DUMP" "ghidra:/tmp/${PROG_NAME}"
            run docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
                /projects legaia -import "/tmp/${PROG_NAME}" \
                -loader BinaryLoader -loader-baseAddr "$BASE" \
                -processor MIPS:LE:32:default -overwrite
            run docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
                /projects legaia -process "${PROG_NAME}"
        fi
    fi

    # Step 3: inventory
    echo "  [3/3] inventory: $PROG_NAME"
    if ! run docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
            /projects legaia -process "${PROG_NAME}" -noanalysis \
            -postScript /scripts/inventory_overlay.py 2>&1 \
            | grep -E "(wrote|ERROR|inventory)"; then
        echo "  WARN: inventory step produced no output line" >&2
    fi

    CSV="ghidra/scripts/inventory_${PROG_NAME}.csv"
    if [[ "$DRY_RUN" -eq 0 && -f "$CSV" ]]; then
        FN_COUNT=$(( $(wc -l < "$CSV") - 1 ))
        echo "  -> $CSV ($FN_COUNT functions)"
    fi

    echo
    PASS=$((PASS + 1))
done

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo "Sweep complete: $PASS/$TOTAL succeeded, $FAIL failed."
echo
if [[ "$DRY_RUN" -eq 0 && "$FAIL" -eq 0 ]]; then
    echo "Inventory CSVs:"
    for LABEL in "${LABELS[@]}"; do
        CSV="ghidra/scripts/inventory_overlay_${LABEL}.bin.csv"
        if [[ -f "$CSV" ]]; then
            FN_COUNT=$(( $(wc -l < "$CSV") - 1 ))
            printf "  %-30s %d functions\n" "$LABEL" "$FN_COUNT"
        else
            printf "  %-30s (not found)\n" "$LABEL"
        fi
    done
fi

[[ "$FAIL" -gt 0 ]] && exit 1
exit 0
