#!/usr/bin/env bash
# record_interactive.sh - launch the interactive input recorder, retrying through
# the resumed-save segfault lottery until the save actually loads, then leave the
# window up to play. The segfault happens early (during the resume, within a few
# seconds); if the emulator is still alive after the warm-up window, the resume
# succeeded and you can play.
#
# Usage:
#   bash scripts/pcsx-redux/record_interactive.sh [scenario] [lua]
# Defaults: scenario = s4_rimelm_door_transition, lua = autorun_record_inputs.lua
#
# Once it prints ">>> RESUMED - PLAY NOW", play (walk to Tetsu, pick the 3rd
# "training fight" option, start the spar). It auto-quits a few seconds after the
# battle starts; or close the window when done. The CSV path is printed by the
# recorder as "[rec] inputs -> .../inputs.csv".

set -u
cd "$(dirname "$0")/../.." || exit 1

SCENARIO="${1:-s4_rimelm_door_transition}"
LUA="${2:-scripts/pcsx-redux/autorun_record_inputs.lua}"
WARMUP_SECS="${WARMUP_SECS:-9}"      # if alive this long, the resume took
MAX_ATTEMPTS="${MAX_ATTEMPTS:-40}"

for attempt in $(seq 1 "$MAX_ATTEMPTS"); do
    echo ">>> launch attempt $attempt/$MAX_ATTEMPTS ..."
    bash scripts/pcsx-redux/run_probe.sh --scenario "$SCENARIO" --lua "$LUA" &
    PID=$!

    died=0
    # poll the warm-up window in 0.5s steps
    steps=$(( WARMUP_SECS * 2 ))
    for _ in $(seq 1 "$steps"); do
        sleep 0.5
        if ! kill -0 "$PID" 2>/dev/null; then died=1; break; fi
    done

    if [ "$died" -eq 0 ]; then
        echo ">>> RESUMED - PLAY NOW (walk to Tetsu, pick the 3rd option = training fight, start the spar)."
        echo ">>> (it auto-quits a few seconds after the battle starts; or close the window when done)"
        wait "$PID"
        echo ">>> recorder exited. CSV is under captures/record_inputs/<newest>/inputs.csv"
        # surface the newest CSV path
        CSV=$(ls -dt captures/record_inputs/*/inputs.csv 2>/dev/null | head -1)
        [ -n "$CSV" ] && echo ">>> CSV: $CSV"
        exit 0
    fi

    wait "$PID" 2>/dev/null
    echo ">>> segfault on resume (lottery) - retrying ..."
done

echo ">>> gave up after $MAX_ATTEMPTS attempts (unusual - the resume usually lands within a handful)."
exit 1
