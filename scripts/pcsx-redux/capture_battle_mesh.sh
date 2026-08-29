#!/usr/bin/env bash
# capture_battle_mesh.sh - patch-to-pixels capture of what the game
# actually builds in RAM for a battle, off a real memory-card load.
#
# Chain: staged disc -> PCSX-Redux cold boot -> CONTINUE -> memory card ->
# forced battle -> 2 MiB RAM dump + framebuffer grab -> offline decode
# against the monster-archive ("enemy table") reference geometry.
#
# Why a memory card and not a save state: a state restores a stale RAM
# image, so it would replay whatever meshes were resident when the state
# was taken - precisely the bytes a disc patch changes. A card load makes
# the game read the assets off the disc under test.
#
# Two staging steps exist because both have burned a run before:
#   1. The disc is staged into a clean directory. PCSX-Redux auto-applies
#      any SIBLING .ppf next to the image it is given (cdrom/ppf.cc), so
#      running straight out of a directory that also holds a patch file
#      silently tests something other than the image named on the command
#      line - and the patcher writes its .ppf beside the input by default.
#   2. The memory card is staged down to a SINGLE save, so the load
#      screen is deterministic and pad injection lands on the same save
#      every time. The copy also means a run can never dirty the
#      original card. Pick a save whose product code ends `-00`: the
#      load grid places a save by its OWN slot number (the code's
#      suffix), not by the card block it sits in, and the cursor starts
#      on slot 1 - anything else needs grid navigation.
#
# Usage:
#   scripts/pcsx-redux/capture_battle_mesh.sh \
#       --iso /path/patched.bin \
#       [--card ~/.config/pcsx-redux/memcard2.mcd] [--keep 14] \
#       [--formation 162] [--out captures/battle_mesh/<stamp>]
#
#   --formation ''  waits for a random encounter instead of forcing one.
#
# List a card's saves first with:
#   scripts/pcsx-redux/isolate_card_save.py --list <card>
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ISO=""
CARD="$HOME/.config/pcsx-redux/memcard2.mcd"
KEEP=1
FORMATION="162"
OUT=""
SETTLE="${LEGAIA_BATTLE_SETTLE:-240}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --iso)       ISO="$2"; shift 2 ;;
        --card)      CARD="$2"; shift 2 ;;
        --keep)      KEEP="$2"; shift 2 ;;
        --formation) FORMATION="$2"; shift 2 ;;
        --out)       OUT="$2"; shift 2 ;;
        --settle)    SETTLE="$2"; shift 2 ;;
        -h|--help)   sed -n '2,33p' "$0"; exit 0 ;;
        *) echo "ERROR: unknown flag: $1" >&2; exit 64 ;;
    esac
done

[[ -n "$ISO" ]] || { echo "ERROR: --iso is required" >&2; exit 64; }
[[ -f "$ISO" ]] || { echo "ERROR: no such disc image: $ISO" >&2; exit 64; }
[[ -f "$CARD" ]] || { echo "ERROR: no such memory card: $CARD" >&2; exit 64; }

STAMP="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
OUT="${OUT:-$REPO_ROOT/captures/battle_mesh/$STAMP}"
WORK="$OUT/stage"
mkdir -p "$WORK"

# ---------- 1. stage the disc, away from any sibling .ppf ----------
DISC="$WORK/disc.bin"
ln -sf "$(readlink -f "$ISO")" "$DISC"
if [[ -e "${DISC%.bin}.ppf" ]]; then
    echo "ERROR: a .ppf sits beside the staged disc; it would be auto-applied" >&2
    exit 1
fi
src_ppf="${ISO%.*}.ppf"
if [[ -e "$src_ppf" ]]; then
    echo "note: '$src_ppf' exists beside the source image; staging bypasses it"
fi

# ---------- 2. stage a single-save memory card ----------
CARD1="$WORK/card1.mcd"
python3 "$REPO_ROOT/scripts/pcsx-redux/isolate_card_save.py" \
    --keep "$KEEP" --out "$CARD1" "$CARD"
# Slot 2 gets its own throwaway card so the run cannot touch a real one.
CARD2="$WORK/card2.mcd"
[[ -f "$CARD2" ]] || head -c $((128 * 1024)) /dev/zero > "$CARD2"

# ---------- 3. run ----------
echo "=== capture_battle_mesh ==="
echo "  disc      : $ISO"
echo "  card      : $CARD (block $KEEP)"
echo "  formation : ${FORMATION:-<random encounter>}"
echo "  out       : $OUT"

# PCSX-Redux always opens a window, so the capture runs on its own X
# server: an unattended run must not steal focus on a live desktop.
# LEGAIA_ON_DISPLAY=1 opts back into the real display for debugging.
XVFB=(xvfb-run -a)
if [[ "${LEGAIA_ON_DISPLAY:-0}" == "1" ]] || ! command -v xvfb-run >/dev/null; then
    XVFB=()
fi

LEGAIA_NO_SSTATE=1 \
LEGAIA_MCD1="$(readlink -f "$CARD1")" \
LEGAIA_MCD2="$(readlink -f "$CARD2")" \
LEGAIA_PCSX_PROFILE_DIR="$WORK/profile" \
LEGAIA_FORMATION="$FORMATION" \
LEGAIA_BATTLE_SETTLE="$SETTLE" \
    "${XVFB[@]}" \
    timeout --kill-after=30s 1200s \
    bash "$REPO_ROOT/scripts/pcsx-redux/run_probe.sh" \
        --lua "$REPO_ROOT/scripts/pcsx-redux/autorun_battle_mesh_dump.lua" \
        --iso "$DISC" \
        --out-dir "$OUT" \
        --isolate-config || echo "(emulator exited non-zero - check $OUT)"

echo
if [[ -f "$OUT/ram.bin" ]]; then
    echo "RAM image: $OUT/ram.bin"
    echo "decode with: python3 scripts/pcsx-redux/decode_battle_mesh.py $OUT/ram.bin"
else
    echo "NO RAM DUMP - read $OUT/dump.log and the pcsx log for the phase it stalled in" >&2
    exit 1
fi
