#!/usr/bin/env bash
# run-mednafen.sh — launch mednafen with a specific save state pre-loaded
# and (optionally) a movie file replaying deterministic input.
#
# Mednafen has no headless mode and no remote-debug protocol, so this
# helper is the simplest path to "boot the game, load a state, replay
# inputs to a known frame, take a fresh state". The .mcm movie file
# captures every controller input from frame 0; replaying it twice
# produces bit-identical RAM at every frame.
#
# Usage:
#   scripts/mednafen/run-mednafen.sh DISC.bin [--state mc1] [--movie m1.mcm]
#                                              [--save-as mc7]
#
# Workflow for capturing a new scenario:
#   1) Boot mednafen on the disc, play to the point you care about.
#   2) F5 to save state into a free slot (e.g. slot 7).
#   3) Run this helper with --state mc7 to verify the slot.
#   4) Optionally record an .mcm movie that replays a short input
#      sequence after slot load (Shift+F5 to start recording).
#   5) Re-run with --movie to replay deterministically — useful when
#      you need to capture states at progressive frames during the
#      same scripted action.
#
# This script does not attempt to drive mednafen via TUI debugger
# breakpoints (that interface is keyboard-only and is not scriptable).
# For breakpoint-equivalent observation use the diff/bisect tools
# in `mednafen-state` against pre/post save states.

set -euo pipefail

if [[ $# -lt 1 ]]; then
    sed -n '2,30p' "$0"
    exit 64
fi

DISC="$1"; shift
STATE_SLOT=""
MOVIE=""
SAVE_AS=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --state) STATE_SLOT="$2"; shift 2 ;;
        --movie) MOVIE="$2"; shift 2 ;;
        --save-as) SAVE_AS="$2"; shift 2 ;;
        *) echo "unknown arg: $1" >&2; exit 64 ;;
    esac
done

if [[ ! -f "$DISC" ]]; then
    echo "disc not found: $DISC" >&2
    exit 66
fi

MEDNAFEN_BIN=$(command -v mednafen || true)
if [[ -z "$MEDNAFEN_BIN" ]]; then
    echo "mednafen not in PATH" >&2
    exit 67
fi

ARGS=()
if [[ -n "$STATE_SLOT" ]]; then
    # Mednafen uses the `psx.state.statefile` config key for save-state
    # slot selection. State slots correspond to the .mc{0..9} files in
    # ~/.mednafen/mcs/.
    SLOT_NUM=${STATE_SLOT#mc}
    ARGS+=(-psx.cdspeedup 0 -loadstate "$SLOT_NUM")
fi
if [[ -n "$MOVIE" ]]; then
    ARGS+=(-mov "$MOVIE")
fi

echo "[run] mednafen ${ARGS[*]} \"$DISC\""
"$MEDNAFEN_BIN" "${ARGS[@]}" "$DISC"

if [[ -n "$SAVE_AS" ]]; then
    SLOT_NUM=${SAVE_AS#mc}
    SAVE_DIR="${LEGAIA_MEDNAFEN_DIR:-$HOME/.mednafen/mcs}"
    HASH=$(ls "$SAVE_DIR" | head -1 | grep -oE '[0-9a-f]{32}' | head -1)
    if [[ -n "$HASH" ]]; then
        SRC="$SAVE_DIR/Legend of Legaia (USA).${HASH}.mc${SLOT_NUM}"
        echo "[ok] save-as -> $SRC"
    fi
fi
