#!/usr/bin/env bash
#
# Two write-watch captures through autorun_w5b_field_watch.lua, kept as one
# runnable recipe so the claims they back can be re-measured.
#
#   offset  drake_castle_to_worldmap: walk out of Drake Castle onto map01 and
#           break on every store to the scene-control word +0x4A - the scene
#           reset's zeroing store (0x8003A0D4) and the three op-0x4C sub-9
#           arms (0x801E14BC delta, 0x801E15B0 player-relative, 0x801E1600
#           default) plus the delta arm's _DAT_8007BCAC store (0x801E14D4).
#           Samples the word, the accumulator and the player's +0x16 per
#           vsync. docs/subsystems/field-ambient-fx.md
#
#   halt    retock_innkeeper_talk_open: run one stay with a Cross cadence,
#           then talk again, write-watching the player's and the innkeeper's
#           +0x10 and breaking on the acquire (0x801E2148) and the walk
#           kernel's two halt-clearing stores (0x80038004 / 0x80038028).
#           The actor addresses are that state's (player 0x80083794,
#           innkeeper 0x80081D6C). docs/subsystems/script-vm.md
#
# Usage: run_w1a_halt_and_offset_watch.sh offset|halt [OUT_DIR]
# Probes do not exit on their own: every run is wrapped in `timeout`.

set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
which="${1:?usage: $0 offset|halt [OUT_DIR]}"
out="${2:-$repo/captures/w1a_${which}_watch}"
mkdir -p "$out"

case "$which" in
    offset)
        scenario=drake_castle_to_worldmap
        frames=500
        export LEGAIA_PRESS="20:UP:40"
        export LEGAIA_EXEC="0x801E14BC:delta_4a,0x801E15B0:rel_4a,0x801E1600:def_4a,0x8003A0D4:reset_4a,0x801E14D4:delta_bcac"
        export LEGAIA_SAMPLE="*0x801C6EA4+0x4A:2:ctl4a,0x8007BCAC:4:bcac,P+0x16:2:foot,0x8007B83C:4:mode"
        ;;
    halt)
        scenario=retock_innkeeper_talk_open
        frames=560
        press=""
        for f in $(seq 20 20 380) 500; do press="${press}${f}:CROSS:4,"; done
        export LEGAIA_PRESS="${press%,}"
        export LEGAIA_WATCH="0x800837A4:4:pflags,0x80081D7C:4:npc10"
        export LEGAIA_EXEC="0x801E2148:acq5,0x80038004:clr_tgt,0x80038028:clr_self,0x8003774C:walk"
        export LEGAIA_SAMPLE="0x800837A4:4:pflags,0x80081D7C:4:npc10,0x800837E8:2:p54,0x800837BA:2:p26"
        export LEGAIA_EXEC_MAX=100
        ;;
    *)
        echo "unknown capture '$which' (offset|halt)" >&2
        exit 2
        ;;
esac

export LEGAIA_FRAMES="$frames" LEGAIA_OUT_DIR="$out"
timeout 900 bash "$here/run_probe.sh" \
    --lua "$here/autorun_w5b_field_watch.lua" \
    --scenario "$scenario" --frames "$frames" --out-dir "$out" --isolate-config
echo "outputs: $out/w5b_writes.csv $out/w5b_exec.csv $out/w5b_samples.csv"
