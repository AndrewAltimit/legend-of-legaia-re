#!/usr/bin/env bash
#
# Fast-recompiler PCSX-Redux probe runner. Same shape as
# run_world_map_probe.sh, but drops the `-interpreter` and `-debugger`
# flags so the emulator runs in its default recompiler mode at ~10-50x
# realtime. Lua GPU::Vsync events still fire under the recompiler.
# Only Lua **breakpoints** require `-interpreter -debugger` (so
# breakpoint-driven probes must keep using run_world_map_probe.sh).
#
# Use this for any probe that only needs vsync events (e.g. RAM
# dumps at specific vsync targets that would otherwise take minutes
# of wall time at interpreter speed).
#
# Environment overrides (all optional): LEGAIA_ISO, LEGAIA_SSTATE,
# LEGAIA_FRAMES, LEGAIA_OUT, LEGAIA_LUA, PCSX_REDUX, LEGAIA_BIOS.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

PCSX_REDUX="${PCSX_REDUX:-$HOME/Tools/pcsx-redux/pcsx-redux}"
LEGAIA_ISO="${LEGAIA_ISO:-$HOME/Downloads/Legend of Legaia (USA)/Legend of Legaia (USA).bin}"
LEGAIA_SSTATE="${LEGAIA_SSTATE:-$HOME/Tools/pcsx-redux/SCUS94254.sstate7}"
LEGAIA_BIOS="${LEGAIA_BIOS:-$HOME/.mednafen/firmware/SCPH1001.BIN}"
LEGAIA_FRAMES="${LEGAIA_FRAMES:-600}"
LEGAIA_OUT="${LEGAIA_OUT:-$REPO_ROOT/fast_probe.csv}"
LEGAIA_LUA="${LEGAIA_LUA:-scripts/pcsx-redux/autorun_dump_full_ram.lua}"

for f in "$PCSX_REDUX" "$LEGAIA_ISO" "$LEGAIA_SSTATE" "$LEGAIA_BIOS"; do
    if [ ! -e "$f" ]; then
        echo "ERROR: required file not found: $f" >&2
        exit 1
    fi
done

mkdir -p "$REPO_ROOT/logs"
LOG_FILE="$REPO_ROOT/logs/pcsx_fast_probe.log"

cd "$REPO_ROOT"

echo "=== run_fast_probe.sh ==="    | tee "$LOG_FILE"
echo "  pcsx-redux : $PCSX_REDUX"    | tee -a "$LOG_FILE"
echo "  bios       : $LEGAIA_BIOS"   | tee -a "$LOG_FILE"
echo "  iso        : $LEGAIA_ISO"    | tee -a "$LOG_FILE"
echo "  sstate     : $LEGAIA_SSTATE" | tee -a "$LOG_FILE"
echo "  lua        : $LEGAIA_LUA"    | tee -a "$LOG_FILE"
echo "  frames     : $LEGAIA_FRAMES" | tee -a "$LOG_FILE"
echo "  out        : $LEGAIA_OUT"    | tee -a "$LOG_FILE"
echo "==========================="   | tee -a "$LOG_FILE"

export LEGAIA_SSTATE LEGAIA_FRAMES LEGAIA_OUT

# Recompiler mode (no -interpreter -debugger). GPU::Vsync events still fire.
stdbuf -oL -eL "$PCSX_REDUX" \
    -bios "$LEGAIA_BIOS" \
    -iso "$LEGAIA_ISO" \
    -run \
    -stdout \
    -dofile "$LEGAIA_LUA" \
    >> "$LOG_FILE" 2>&1
EXIT=$?

echo "" | tee -a "$LOG_FILE"
echo "pcsx-redux exited with status $EXIT" | tee -a "$LOG_FILE"
exit "$EXIT"
