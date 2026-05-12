#!/usr/bin/env bash
#
# Closed-loop world-map VM probe runner.
#
# Launches the locally-built PCSX-Redux ARM64 binary with:
#   - interpreter CPU (so Lua breakpoints actually fire)
#   - the Legaia disc image preloaded
#   - the autorun Lua script that loads a save state, arms the probe
#     breakpoints, captures N VSyncs, writes a CSV, and quits.
#
# Environment overrides (all optional):
#   LEGAIA_ISO     path to Legend of Legaia (USA).bin
#   LEGAIA_SSTATE  path to PCSX-Redux save state (.sstate file)
#   LEGAIA_FRAMES  number of post-load VSyncs to capture (default 600 = ~10s)
#   LEGAIA_OUT     CSV output path
#   PCSX_REDUX     path to the pcsx-redux binary
#
# Output goes to:
#   - CSV at LEGAIA_OUT (default: world_map_probe.csv in the repo root)
#   - emulator log at logs/pcsx_world_map_probe.log

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

PCSX_REDUX="${PCSX_REDUX:-$HOME/Tools/pcsx-redux/pcsx-redux}"
LEGAIA_ISO="${LEGAIA_ISO:-$HOME/Downloads/Legend of Legaia (USA)/Legend of Legaia (USA).bin}"
LEGAIA_SSTATE="${LEGAIA_SSTATE:-$HOME/Tools/pcsx-redux/SCUS94254.sstate1}"
# PCSX-Redux only finds the BIOS via persistent config when launched from
# its portable dir (~/Tools/pcsx-redux, which has pcsx.json). Launching
# from the repo root means -no-portable mode, so we have to pass -bios
# explicitly. The .mednafen firmware dir already has SCPH1001.BIN.
LEGAIA_BIOS="${LEGAIA_BIOS:-$HOME/.mednafen/firmware/SCPH1001.BIN}"
LEGAIA_FRAMES="${LEGAIA_FRAMES:-600}"
LEGAIA_OUT="${LEGAIA_OUT:-$REPO_ROOT/world_map_probe.csv}"
LEGAIA_LUA="${LEGAIA_LUA:-scripts/pcsx-redux/autorun_world_map_probe.lua}"

for f in "$PCSX_REDUX" "$LEGAIA_ISO" "$LEGAIA_SSTATE" "$LEGAIA_BIOS"; do
    if [ ! -e "$f" ]; then
        echo "ERROR: required file not found: $f" >&2
        exit 1
    fi
done

mkdir -p "$REPO_ROOT/logs"
LOG_FILE="$REPO_ROOT/logs/pcsx_world_map_probe.log"

cd "$REPO_ROOT"

echo "=== run_world_map_probe.sh ===" | tee "$LOG_FILE"
echo "  pcsx-redux : $PCSX_REDUX"     | tee -a "$LOG_FILE"
echo "  bios       : $LEGAIA_BIOS"    | tee -a "$LOG_FILE"
echo "  iso        : $LEGAIA_ISO"     | tee -a "$LOG_FILE"
echo "  sstate     : $LEGAIA_SSTATE"  | tee -a "$LOG_FILE"
echo "  lua        : $LEGAIA_LUA"     | tee -a "$LOG_FILE"
echo "  frames     : $LEGAIA_FRAMES"  | tee -a "$LOG_FILE"
echo "  csv out    : $LEGAIA_OUT"     | tee -a "$LOG_FILE"
echo "==============================" | tee -a "$LOG_FILE"

export LEGAIA_SSTATE LEGAIA_FRAMES LEGAIA_OUT

# stdbuf forces line-buffered stdout/stderr so the log streams live rather
# than dumping in one chunk at exit (matters when we kill the run on
# timeout). pcsx-redux's `-stdout` enables its fputs-to-stdout path.
# Two-flag requirement: -interpreter selects the non-recompiling CPU, but
# the interpreter only invokes Debug::process (the breakpoint check) when
# DebugSettings::Debug is also on. -debugger flips that. Without it,
# Lua breakpoints silently never fire even in interpreter mode.
# See psxinterpreter.cc:1652 (`if constexpr (debug)`).
stdbuf -oL -eL "$PCSX_REDUX" \
    -interpreter \
    -debugger \
    -bios "$LEGAIA_BIOS" \
    -iso "$LEGAIA_ISO" \
    -run \
    -stdout \
    -dofile "$LEGAIA_LUA" \
    >> "$LOG_FILE" 2>&1
EXIT=$?

echo "" | tee -a "$LOG_FILE"
echo "pcsx-redux exited with status $EXIT" | tee -a "$LOG_FILE"

if [ -f "$LEGAIA_OUT" ]; then
    rows=$(($(wc -l < "$LEGAIA_OUT") - 1))
    echo "CSV: $LEGAIA_OUT  ($rows sample rows)"
fi

# Pull out the probe-hits summary from the log for quick visual.
echo ""
echo "=== probe hits summary ==="
grep -A 10 "=== world-map probe hits ===" "$LOG_FILE" | head -12 || true

exit "$EXIT"
