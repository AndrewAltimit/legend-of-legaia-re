# @category Legaia
# @runtime Jython
#
# Dumps the title-overlay tick function (and its caller) from
# overlay_title.bin.
#
# overlay_title.bin captured from PCSX-Redux sstate8 via
# autorun_countdown_trigger.lua. The watchpoint at 0x801EF16C (title-
# attract countdown) fired once per frame; the BP captured the PC of
# the title overlay's per-frame tick function exactly at the decrement
# instruction.
#
# Captured registers (captures/boot_walk/overlay_title.bin.regs):
#   pc 0x801DDCCC  - tick instruction that decrements the countdown
#   ra 0x801DD6B8  - caller (a frame outer-loop or game-mode dispatcher)
#   a0 0x801F0000  - struct base passed in (title-overlay state? or BSS?)
#   sp 0x801FFDE0  - stack near top of overlay window
#   gp 0x8007B318  - SCUS-side globals
#
# Run against the named overlay program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_title.bin -noanalysis \
#       -postScript /scripts/dump_title_overlay.py
#
# Output files land in /scripts/funcs/overlay_title_<addr>.txt

from lib_dump import dump_targets

TARGETS = [
    # Pinned from the watchpoint capture.
    "801ddccc",  # tick instruction that decrements 0x801EF16C
    "801dd6b8",  # caller (RA)
]

dump_targets(currentProgram, TARGETS)
