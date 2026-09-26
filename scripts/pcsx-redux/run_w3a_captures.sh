#!/usr/bin/env bash
#
# Three retail captures through autorun_w5b_field_watch.lua, kept as one
# runnable recipe so the claims they back can be re-measured.
#
#   opdeene     s1_newgame_field run forward through the first narration
#               crawl: screenshots + raw checkpoints at vsyncs where the
#               jungle is drawn. Read offline: the resident TMDs' colour words
#               (DAT_8007C018) and the frame's ordering table
#               (scripts/mednafen/display-list.py on the extracted RAM).
#               docs/tooling/host-drift.md "Gaps absent from both hosts".
#
#   baka_cameo  baka_fighter_entry_pretransition, Triangle held from vsync
#               200, START at 700, Cross at 820 / 910 / 1000 / 1090: the
#               round setup spawns the cameo (FUN_801D6310, first hit near
#               vsync 885). Samples the cameo actor's phase / clip / model /
#               x / yaw, and checkpoints around the pose; sstate_vram.py pulls
#               VRAM out of each so the MoveImage cell at (0x340, 0x86) can be
#               compared with its two sources.
#               docs/subsystems/minigame-baka-fighter.md "The round-start cameo".
#
#   dome        baka_fighter_entry_pretransition with the warp's sub-id
#               (0x8007BA34, u16) re-poked from 4 to 5 through the warp
#               window, so the mode-24 init streams the arena (PROT 0977)
#               instead of the Baka overlay; a Cross cadence then starts a
#               round. Exec-breaks the VAB loader / closer, the mode
#               initialiser and the warp, and samples the slot-2 / slot-6
#               enable bytes (0x80091508 + slot*12 + 0xB) and the field-bank
#               latch 0x8007BAFC. docs/subsystems/audio.md.
#
# That state's resident SCUS carries the new-game starting-bag seed (a patch
# site outside the minigame, VAB and field-render paths); the disc under the
# run is the staged retail image (run_probe.sh never hands PCSX-Redux a path
# with a sibling .ppf).
#
# Usage: run_w3a_captures.sh opdeene|baka_cameo|dome [OUT_DIR]
# Probes do not exit on their own: every run is wrapped in `timeout`.

set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
which="${1:?usage: $0 opdeene|baka_cameo|dome [OUT_DIR]}"
out="${2:-$repo/captures/w3a_${which}}"
mkdir -p "$out"

case "$which" in
    opdeene)
        scenario=s1_newgame_field
        frames=1460
        export LEGAIA_SHOTS="600,750,1000,1150,1300,1450"
        export LEGAIA_CKPTS="600,750,1000,1150,1300,1450"
        ;;
    baka_cameo)
        scenario=baka_fighter_entry_pretransition
        frames=1120
        press="200:TRIANGLE:1000,700:START:6"
        for f in 820 910 1000 1090; do press="${press},${f}:CROSS:6"; done
        export LEGAIA_PRESS="$press"
        export LEGAIA_SHOTS="880,900,940,980,1020,1060,1100"
        export LEGAIA_CKPTS="900,960,1040"
        export LEGAIA_EXEC="0x801D6310:cameo,0x80020DE0:spawn"
        export LEGAIA_EXEC_MAX=3
        export LEGAIA_WATCH="0x800832E8:2:model"
        export LEGAIA_SAMPLE="0x800832A6:2:phase,0x800832E0:2:clip,0x800832E8:2:model,0x80083298:2:x,0x800832AA:2:yaw"
        ;;
    dome)
        scenario=baka_fighter_entry_pretransition
        frames=2000
        poke=""
        for f in $(seq 120 4 480); do poke="${poke}${f}:0x8007BA34:2:5,"; done
        press=""
        for f in $(seq 700 60 1900); do press="${press}${f}:CROSS:6,"; done
        export LEGAIA_MEMPOKE="${poke%,}"
        export LEGAIA_PRESS="${press%,}"
        export LEGAIA_SHOTS="600,1000,1400,1900"
        export LEGAIA_CKPTS="600,1100,1500,1900"
        export LEGAIA_EXEC="0x8001FC00:vabload,0x8001FF58:vabclose,0x8001DCF8:modeinit,0x80025980:warp"
        export LEGAIA_EXEC_MAX=40
        export LEGAIA_WATCH="0x8007BAFC:4:latch"
        export LEGAIA_SAMPLE="0x8007B83C:1:mode,0x8007BA34:2:sub,0x8009152B:1:en2,0x8009155B:1:en6,0x8007BAFC:4:latch"
        ;;
    *)
        echo "unknown capture '$which' (opdeene|baka_cameo|dome)" >&2
        exit 2
        ;;
esac

export LEGAIA_FRAMES="$frames" LEGAIA_OUT_DIR="$out"
timeout 900 bash "$here/run_probe.sh" \
    --lua "$here/autorun_w5b_field_watch.lua" \
    --scenario "$scenario" --frames "$frames" --out-dir "$out" --isolate-config
echo "outputs: $out (w5b_*.csv, shot_*.raw, ckpt_*.rawsstate)"
