#!/usr/bin/env bash
#
# Gaza 2 magic-softlock reproduction sweep.
#
# The community softlock (endless battle-camera orbit after a player magic
# cast) is RNG-gated and rare, so one run proves nothing. This driver replays
# the SAME Gaza 2 save state N times through autorun_gaza2_magic_wedge.lua,
# varying the AUTOPILOT CADENCE - the vsync period at which the probe presses
# the next button of its repeating macro - one attempt per cadence.
#
# Why the cadence and not a timing jitter: PCSX-Redux resuming a fixed save
# state with a fixed input script is DETERMINISTIC, so a replay samples
# nothing. A +/-1 vsync jitter does not help either - it is absorbed by the
# menu's edge detection, and MEASURED jittered replays reproduce the same
# 1224-vsync summon dwell exactly, digit for digit. Changing the cadence
# instead changes which menu entry each press lands on, so the action MIX
# changes (magic vs attack vs spirit, different targets), the enemy gets
# different turns, and the RNG call count actually diverges.
#
# Every attempt records its per-state max dwell, so the sweep yields a dwell
# DISTRIBUTION for the summon band even when the wedge never fires - a
# no-repro run is still a measurement.
#
# A stall is flagged by the probe only inside the cast bands (0x28..0x2E /
# 0x32..0x38) and only past --stall-n. On this save a HEALTHY summon state
# 0x36 measures ~1224 vsyncs, so the default threshold sits well above that.
#
# Usage:
#   bash scripts/pcsx-redux/run_gaza2_wedge_sweep.sh \
#       --attempts 10 --sstate ~/.config/pcsx-redux/SCUS94254.sstate9
#
# Flags:
#   --attempts N     number of cadences to try (default 8)
#   --cadences LIST  comma-separated autopilot vsync periods to sweep
#                    (default 45,37,53,29,61,41,49,33)
#   --sstate PATH    the Gaza 2 save state (fingerprint it first with
#                    analyze_gaza2_fingerprint.py - never trust a slot number)
#   --frames N       capture vsyncs per attempt (default 4200)
#   --stall-n N      cast-band dwell that counts as a wedge (default 1800)
#   --out DIR        sweep output root (default captures/gaza2_wedge_sweep/<ts>)
#   --timeout SEC    per-attempt kill timeout (default 900). PCSX-Redux probes
#                    do NOT reliably self-quit; the timeout is mandatory.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# proc_spawn_group / proc_wait_pid / proc_kill_group. Each attempt runs in its
# OWN process group so the deadline can kill the emulator too. Plain
# `timeout <n> bash run_probe.sh` cannot: timeout signals its direct child, the
# shell dies, and PCSX-Redux is orphaned - observed live, an orphan from a
# finished run kept burning CPU and slowed the next attempt to a crawl. These
# helpers also make the liveness check structurally unable to match this script.
# shellcheck source=../lib/proc.sh
source "$REPO_ROOT/scripts/lib/proc.sh"

ATTEMPTS=8
SSTATE="$HOME/.config/pcsx-redux/SCUS94254.sstate9"
FRAMES=4200
STALL_N=1800
OUT_ROOT=""
PER_TIMEOUT=900
CADENCES="45,37,53,29,61,41,49,33"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --attempts) ATTEMPTS="$2"; shift 2 ;;
        --cadences) CADENCES="$2"; shift 2 ;;
        --sstate)   SSTATE="$2";   shift 2 ;;
        --frames)   FRAMES="$2";   shift 2 ;;
        --stall-n)  STALL_N="$2";  shift 2 ;;
        --out)      OUT_ROOT="$2"; shift 2 ;;
        --timeout)  PER_TIMEOUT="$2"; shift 2 ;;
        -h|--help)  sed -n '2,40p' "$0"; exit 0 ;;
        *) echo "ERROR: unknown flag: $1" >&2; exit 64 ;;
    esac
done

if [[ ! -e "$SSTATE" ]]; then
    echo "ERROR: save state not found: $SSTATE" >&2
    exit 1
fi

if [[ -z "$OUT_ROOT" ]]; then
    OUT_ROOT="$REPO_ROOT/captures/gaza2_wedge_sweep/$(date -u +%Y-%m-%dT%H-%M-%SZ)"
fi
mkdir -p "$OUT_ROOT"

IFS=',' read -r -a CADENCE_ARR <<< "$CADENCES"

SUMMARY="$OUT_ROOT/sweep.tsv"
printf 'attempt\tcadence\tstalled\tcast_band_max_dwell\tstates_visited\n' > "$SUMMARY"

echo "=== gaza2 wedge sweep ==="
echo "  sstate   : $SSTATE"
echo "  attempts : $ATTEMPTS"
echo "  frames   : $FRAMES   stall_n: $STALL_N"
echo "  out      : $OUT_ROOT"
echo "========================="

hits=0
for (( i = 0; i < ATTEMPTS; i++ )); do
    cadence="${CADENCE_ARR[$(( i % ${#CADENCE_ARR[@]} ))]}"

    dir="$OUT_ROOT/attempt_$(printf '%02d' "$i")"
    mkdir -p "$dir"
    echo "--- attempt $i (autopilot cadence ${cadence} vsyncs) -> $dir"

    # Own process group + explicit deadline. PCSX-Redux does not reliably exit
    # on its own, and killing only the wrapper strands it.
    set +e
    pgid=$(LEGAIA_AUTOPILOT="$cadence" \
           LEGAIA_STALL_N="$STALL_N" \
           LEGAIA_OUT_DIR="$dir" \
           proc_spawn_group "$dir/runner.log" \
               bash "$REPO_ROOT/scripts/pcsx-redux/run_probe.sh" \
               --lua "$REPO_ROOT/scripts/pcsx-redux/autorun_gaza2_magic_wedge.lua" \
               --sstate "$SSTATE" --frames "$FRAMES" --out-dir "$dir")
    proc_wait_pid "$pgid" "$PER_TIMEOUT"
    waited=$?
    set -e
    if [[ $waited -ne 0 ]]; then
        echo "    WARN: attempt $i hit the ${PER_TIMEOUT}s deadline; killing its group"
    fi
    # Unconditional: even a probe that finished its capture can leave the
    # emulator up (observed), so tear the group down either way.
    proc_kill_group "$pgid"

    stalled="?"
    dwell="?"
    states=""
    if [[ -f "$dir/summary.txt" ]]; then
        # grep exits 1 on no-match; that is not a failure here, so each read is
        # guarded with `|| true` rather than riding the pipeline's status.
        stalled=$(sed -n 's/^stall dumped: *//p' "$dir/summary.txt" | head -1)
        dwell=$(sed -n 's/^cast-band max dwell: *//p' "$dir/summary.txt" | head -1)
        states=$(sed -n 's/^ctx+7 states visited: *//p' "$dir/summary.txt" | head -1)
    fi
    [[ -z "$stalled" ]] && stalled="?"
    [[ -z "$dwell" ]] && dwell="?"

    printf '%d\t%s\t%s\t%s\t%s\n' "$i" "$cadence" "$stalled" "$dwell" "$states" >> "$SUMMARY"
    echo "    stalled=$stalled cast-band max dwell=$dwell"

    if [[ "$stalled" == "true" ]]; then
        hits=$((hits + 1))
        echo "    *** WEDGE CANDIDATE: see $dir/stall.txt"
    fi
done

echo ""
echo "=== sweep done: $hits/$ATTEMPTS attempt(s) flagged a cast-band stall ==="
echo "summary: $SUMMARY"
column -t -s $'\t' "$SUMMARY" || cat "$SUMMARY"
