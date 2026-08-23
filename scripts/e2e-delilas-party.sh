#!/usr/bin/env bash
#
# End-to-end test for the --delilas-party mod: patch a fresh rom from the
# retail disc, statically verify the rom's build invariants
# (`legaia-patcher delilas-verify`), then run a live emulator pass - load
# a pre-encounter field state, roll a NATURAL encounter, drive every
# round's command ring to Spirit - and summarize the verdicts.
#
# The two stages answer different questions:
#   static  - "is the current code's output on this disc?" (catches a rom
#             built by a stale patcher: cached wasm, old server)
#   live    - "does a fresh battle on this disc load, idle, and Spirit
#             without artifacts?" (ordinal telemetry + screenshots; old
#             save states can NOT answer this - they carry the RAM-cached
#             assembly from the disc they were made on)
#
# Usage:
#   bash scripts/e2e-delilas-party.sh                  # patch fresh + both stages
#   bash scripts/e2e-delilas-party.sh --rom PATH       # test an existing rom
#   bash scripts/e2e-delilas-party.sh --skip-live      # static stage only
#
# Flags:
#   --rom PATH        verify + live-run an existing patched rom (skips patching)
#   --mapping M       --delilas-party mapping (default lu,gi,che)
#   --out-dir DIR     output root (default captures/delilas-e2e/<iso-ts>)
#   --frames N        live-stage capture vsyncs (default 20000)
#   --scenario NAME   pre-encounter state (default karisto_sol_pre_encounter)
#   --skip-live       static verdict only
#
# Requires: LEGAIA_DISC_BIN (unless --rom), target/release binaries,
# the local PCSX-Redux build + the scenario state for the live stage.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ROM=""
MAPPING="lu,gi,che"
OUT_DIR=""
FRAMES=20000
SCENARIO="karisto_sol_pre_encounter"
SKIP_LIVE=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --rom) ROM="$2"; shift 2 ;;
        --mapping) MAPPING="$2"; shift 2 ;;
        --out-dir) OUT_DIR="$2"; shift 2 ;;
        --frames) FRAMES="$2"; shift 2 ;;
        --scenario) SCENARIO="$2"; shift 2 ;;
        --skip-live) SKIP_LIVE=1; shift ;;
        *) echo "unknown flag: $1" >&2; exit 2 ;;
    esac
done

PATCHER="${REPO}/target/release/legaia-patcher"
[[ -x "$PATCHER" ]] || { echo "ERROR: build first (cargo build --release -p legaia-patcher)" >&2; exit 1; }
[[ -n "$OUT_DIR" ]] || OUT_DIR="${REPO}/captures/delilas-e2e/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$OUT_DIR"

# --- Stage 0: the rom under test -------------------------------------------
if [[ -z "$ROM" ]]; then
    : "${LEGAIA_DISC_BIN:?set LEGAIA_DISC_BIN or pass --rom}"
    ROM="${OUT_DIR}/delilas_e2e.bin"
    echo "[e2e] patching fresh rom (--delilas-party ${MAPPING}) -> ${ROM}"
    # The PPF is directed INTO the out dir AND named unlike the .bin: the
    # default lands beside the RETAIL disc, and PCSX-Redux auto-applies a
    # same-stem sibling .ppf on every load of a disc - two live traps
    # (never leave one beside the retail disc, never name one after the
    # patched image it would then re-patch).
    "$PATCHER" randomize \
        --input "$LEGAIA_DISC_BIN" \
        --seed 1 \
        --delilas-party "$MAPPING" \
        --patch "${OUT_DIR}/patch-vs-retail.ppf" \
        --output "$ROM" \
        > "${OUT_DIR}/patch.log" 2>&1 || { tail -20 "${OUT_DIR}/patch.log"; exit 1; }
    tail -5 "${OUT_DIR}/patch.log"
fi

# --- Stage 1: static verdict -----------------------------------------------
echo "[e2e] static verdict: delilas-verify"
if ! "$PATCHER" delilas-verify --input "$ROM" | tee "${OUT_DIR}/verify.log"; then
    echo "[e2e] STATIC FAIL - the rom does not carry the current build. Stop." >&2
    exit 1
fi

if [[ "$SKIP_LIVE" == "1" ]]; then
    echo "[e2e] static PASS (live stage skipped)"
    exit 0
fi

# --- Stage 2: live verdict --------------------------------------------------
LIVE_DIR="${OUT_DIR}/live"
# SCUS re-sync: the scenario save state carries the resident SCUS of the
# disc it was made on; the probe must poke the patched disc's SCUS diff
# over it after load, or fresh overlay code faults in a stale injection
# arena (dynarec "Unknown instruction" at 0x8007782C, battle wedged).
if [[ -n "${LEGAIA_DISC_BIN:-}" ]]; then
    "$PATCHER" scus-pokes --patched "$ROM" --baseline "$LEGAIA_DISC_BIN" \
        > "${OUT_DIR}/scus-pokes.txt"
    export LEGAIA_POKES_FILE="${OUT_DIR}/scus-pokes.txt"
    echo "[e2e] scus re-sync: $(wc -l < "${OUT_DIR}/scus-pokes.txt") pokes"
else
    echo "[e2e] WARN: LEGAIA_DISC_BIN unset - no SCUS re-sync pokes; a stale-state arena fault is possible"
fi
echo "[e2e] live stage: ${SCENARIO} on ${ROM} (${FRAMES} vsyncs, --fast)"
bash "${REPO}/scripts/pcsx-redux/run_probe.sh" \
    --fast \
    --lua "${REPO}/scripts/pcsx-redux/autorun_delilas_e2e.lua" \
    --scenario "$SCENARIO" \
    --iso "$ROM" \
    --frames "$FRAMES" \
    --out-dir "$LIVE_DIR" \
    > "${OUT_DIR}/live.log" 2>&1 || { tail -20 "${OUT_DIR}/live.log"; exit 1; }

CSV="${LIVE_DIR}/delilas_e2e.csv"
[[ -f "$CSV" ]] || { echo "[e2e] LIVE FAIL - no probe CSV at ${CSV}" >&2; exit 1; }

# Convert screenshots for the eye.
if compgen -G "${LIVE_DIR}/*.raw" > /dev/null; then
    python3 "${REPO}/scripts/pcsx-redux/raw2png.py" "${LIVE_DIR}"/*.raw > /dev/null 2>&1 || true
fi

# --- Verdict summary --------------------------------------------------------
fail=0
if grep -q ",battle-reached" "$CSV"; then
    echo "[e2e] PASS battle-reached (fresh battle loaded from the patched disc)"
else
    echo "[e2e] FAIL battle never reached - see ${CSV}"; fail=1
fi
done_row="$(grep ",done " "$CSV" | tail -1 || true)"
echo "[e2e] ${done_row:-no done row (probe cut short)}"
case "$done_row" in
    *"spirit_frames=0"*|"") echo "[e2e] FAIL no Spirit clip observed"; fail=1 ;;
    *) echo "[e2e] PASS Spirit performed" ;;
esac
# Ordinal telemetry is informational: the ctx+0x240 array may index
# battle entities beyond the player slots (a retail monster block
# legitimately carries 0xFE extras), so an absolute threshold cannot
# judge it. Compare against a known-good run of the same scenario.
echo "[e2e] ordinal telemetry:"
{ grep ",ordinals " "$CSV" || true; } | tail -5 | sed 's/^/[e2e]   /'
echo "[e2e] outputs: ${OUT_DIR}"
echo "[e2e]   screenshots: $(ls "${LIVE_DIR}"/*.png 2>/dev/null | wc -l) png"
exit $fail
