# @category Legaia
# @runtime Jython
#
# Provenance-gap backfill dumps: the seven addresses `port-catalog.py
# --dashboard` reports as ported but not dumped. Per-program target lists
# keyed on the current program's name, following the
# dump_pending_helpers.py pattern (in_program() guard + out_path_for()
# prefix).
#
# Six of the seven are PROT 0977 (arena_init / "other_game", the Muscle
# Dome hub) functions ported in engine-core::muscle_dome and
# engine-ui::other_game_hud; they must be dumped from the slot-A-based
# 0977 import, NOT from the overlay_muscle_dome capture (whose slot A
# holds the battle-action overlay 0898 - VA aliasing, see
# docs/tooling/dump-corpus-integrity.md). The seventh is the
# field-to-battle transition init's style-selection block, resident in
# the field_battle_intro capture.
#
# Run against each named program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process 0977_other_game.BIN -noanalysis \
#       -postScript /scripts/dump_provenance_gap_0729.py
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_field_battle_intro.bin -noanalysis \
#       -postScript /scripts/dump_provenance_gap_0729.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# program name -> (output label, [target VAs])
PROGRAM_TARGETS = {
    # PROT 0977 at its true slot-A base 0x801CE818.
    "0977_other_game.BIN": ("overlay_0977_slotA", [
        "801cf074",  # tally screen HP-restore lanes (muscle_dome::hp_restore)
        "801cf870",  # hub state machine (muscle_dome cursor repack / restore_hp states)
        "801d1184",  # between-leg tally rows (muscle_dome::leg_score_rows)
        "801d1510",  # course-ladder walk (muscle_dome::parse_course_ladder)
        "801d02f0",  # ROUND banner (other_game_hud::round_banner_draws)
        "801d15c8",  # hub screen quads (other_game_hud::hub_screen_quads)
        "801d00f8",  # hub SM callee (cited by the 801cf870 dump)
        "801d042c",  # hub SM callee (cited by the 801cf870 dump)
        "801d1610",  # callee cited by the 801d00f8 dump
    ]),
    # Field->battle transition capture; the style-selection block of the
    # transition init (engine-vm::battle_intro_styles::select_intro_style).
    # The historical name FUN_801CE8C0 is mis-rounded: in the static 0979
    # image 0x801CE8C0..0x801CE8C8 are three data words (1, 0x11, 0x12) and
    # the one prologue before the 0x801CE97C style block is at 0x801CE8CC.
    "overlay_field_battle_intro.bin": ("overlay_field_battle_intro", [
        "801ce8cc",
    ]),
}

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)

entry = PROGRAM_TARGETS.get(prog_name)


def out_path_for(label, addr_str):
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def dump(label, addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        print("[skip] {} not in {}".format(addr_str, prog_name))
        return
    func = fm.getFunctionAt(addr) or fm.getFunctionContaining(addr)
    if func is None:
        disassemble(addr)
        func = createFunction(addr, "FUN_" + addr_str)
    if func is None:
        func = fm.getFunctionContaining(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(label, addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{} base-tagged] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    print("wrote {}".format(out_path))


if entry is None:
    print("[skip] no targets for program {}".format(prog_name))
else:
    label, targets = entry
    for t in targets:
        dump(label, t)

print("done [{}]".format(prog_name))
