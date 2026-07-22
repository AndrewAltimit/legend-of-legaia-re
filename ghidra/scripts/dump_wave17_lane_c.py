# @category Legaia
# @runtime Jython
#
# Wave-17 lane C dumps. Per-program target lists keyed on the current
# program's name, following the dump_pending_helpers.py pattern
# (in_program() guard + out_path_for() prefix). Used to re-derive
# correct-base dumps for functions whose only prior dumps came from the
# mis-based 0x801C0000-band imports of PROT 0977/0978 (see
# docs/tooling/dump-corpus-integrity.md - a dump's printed addresses are
# a property of the load base).
#
# Run against a named program, e.g.:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process 0977_other_game.BIN -noanalysis \
#       -postScript /scripts/dump_wave17_lane_c.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# program name -> (output label, [target VAs])
PROGRAM_TARGETS = {
    # PROT 0977 imported at its true slot-A base 0x801CE818 (string anchors
    # into its own monster-name table pin the base; the overlay_0977_other_game
    # program at 0x801C0000 is mis-based). 801d0f60 = the minigame completion
    # reward previously mis-cited as FUN_801C2748 (file +0x2748).
    "0977_other_game.BIN": ("overlay_0977_slotA", [
        "801d0f60",
    ]),
    # PROT 0898 at its verified base 0x801CE818. Targets re-derive the
    # 801f452c alias census: the real entry FUN_801F452C (magic-level-up
    # message composer), its neighbour FUN_801F45A4, and the two true-VA
    # containers of the wrong-base "801f452c" fragments printed by the
    # 0x801C0000-band overlay_0896/overlay_0897 imports (interiors at
    # 0x801D4D44 -> FUN_801D388C and 0x801DDD44 -> entry near 0x801DDB30).
    "overlay_battle_action_0898.bin": ("overlay_0898_static", [
        "801f452c",
        "801f45a4",
        "801d4d44",
        "801ddd44",
        "801ddb30",
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
