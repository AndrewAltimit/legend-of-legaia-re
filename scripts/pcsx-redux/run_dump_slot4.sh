#!/usr/bin/env bash
#
# Closed-loop slot-4 RAM dumper.
#
# Launches PCSX-Redux, loads the requested save state, waits for the
# kingdom data to settle, dumps the live slot-4 RAM bytes to a .bin,
# and quits. Output can then be byte-compared against the disc-decoded
# slot 4 (see scripts/pcsx-redux/diff_slot4_ram_vs_disc.py).
#
# Env overrides (all optional):
#   LEGAIA_ISO      path to Legend of Legaia (USA).bin
#   LEGAIA_SSTATE   path to PCSX-Redux save state (.sstate file)
#                   default: ~/Tools/pcsx-redux/SCUS94254.sstate2
#                   (the user's map-overview Drake state)
#   LEGAIA_KINGDOM  drake | sebucus | karisto  (default: drake)
#   LEGAIA_OUT      output .bin path  (default: slot4_ram_<kingdom>.bin)
#   LEGAIA_FRAMES   post-load vsyncs to wait before reading (default 120)
#   PCSX_REDUX      path to the pcsx-redux binary
#   LEGAIA_BIOS     SCPH1001.BIN path

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

PCSX_REDUX="${PCSX_REDUX:-$HOME/Tools/pcsx-redux/pcsx-redux}"
LEGAIA_ISO="${LEGAIA_ISO:-$HOME/Downloads/Legend of Legaia (USA)/Legend of Legaia (USA).bin}"
LEGAIA_SSTATE="${LEGAIA_SSTATE:-$HOME/Tools/pcsx-redux/SCUS94254.sstate2}"
LEGAIA_BIOS="${LEGAIA_BIOS:-$HOME/.mednafen/firmware/SCPH1001.BIN}"
LEGAIA_KINGDOM="${LEGAIA_KINGDOM:-drake}"
LEGAIA_FRAMES="${LEGAIA_FRAMES:-120}"
LEGAIA_OUT="${LEGAIA_OUT:-$REPO_ROOT/slot4_ram_${LEGAIA_KINGDOM}.bin}"
LEGAIA_LUA="${LEGAIA_LUA:-scripts/pcsx-redux/autorun_dump_slot4.lua}"

for f in "$PCSX_REDUX" "$LEGAIA_ISO" "$LEGAIA_SSTATE" "$LEGAIA_BIOS"; do
    if [ ! -e "$f" ]; then
        echo "ERROR: required file not found: $f" >&2
        exit 1
    fi
done

mkdir -p "$REPO_ROOT/logs"
LOG_FILE="$REPO_ROOT/logs/pcsx_dump_slot4.log"

cd "$REPO_ROOT"

echo "=== run_dump_slot4.sh ===" | tee "$LOG_FILE"
echo "  pcsx-redux : $PCSX_REDUX"      | tee -a "$LOG_FILE"
echo "  bios       : $LEGAIA_BIOS"     | tee -a "$LOG_FILE"
echo "  iso        : $LEGAIA_ISO"      | tee -a "$LOG_FILE"
echo "  sstate     : $LEGAIA_SSTATE"   | tee -a "$LOG_FILE"
echo "  kingdom    : $LEGAIA_KINGDOM"  | tee -a "$LOG_FILE"
echo "  frames     : $LEGAIA_FRAMES"   | tee -a "$LOG_FILE"
echo "  out        : $LEGAIA_OUT"      | tee -a "$LOG_FILE"
echo "==============================="  | tee -a "$LOG_FILE"

export LEGAIA_SSTATE LEGAIA_KINGDOM LEGAIA_FRAMES LEGAIA_OUT

stdbuf -oL -eL "$PCSX_REDUX" \
    -interpreter \
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
    size=$(stat -c %s "$LEGAIA_OUT")
    echo "Dumped slot 4: $LEGAIA_OUT  ($size bytes)"
else
    echo "WARNING: $LEGAIA_OUT was not written"
fi

# Surface key lines from the log.
echo ""
echo "=== dump_slot4 log highlights ==="
grep "\[dump_slot4\]" "$LOG_FILE" | head -20 || true

exit "$EXIT"
