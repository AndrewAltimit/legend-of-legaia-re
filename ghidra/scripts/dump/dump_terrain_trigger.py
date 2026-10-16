# @category Legaia
# @runtime Jython
#
# Dump FUN_801D8258 - the function that writes _DAT_801F351C, the
# one-shot gate flag for the world-map continent terrain emitter
# FUN_801D7EA0. Found by find_terrain_emitter_caller.py.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801D8258",  # arms _DAT_801F351C
    "801D1344",  # outer caller in world_map (calls FUN_801D8258 at PC 0x801D1470)
    "80016444",  # SCUS-resident outer caller of FUN_801D7EA0 (direct jal at 0x80016764)
    "80025EEC",  # SCUS caller of FUN_80016444
    "80025F2C",  # SCUS caller of FUN_80016444 (sibling)
    "80016230",  # SCUS game_mode setter (writes 0x8007BC3C 2x)
    "801C2B2C",  # field-overlay caller of gate setter (jal 0x801D8258 at 0x801C2C58)
]

PROGRAM_NAME = currentProgram.getName()
OUT_DIR = "/scripts/funcs"
PREFIX = PROGRAM_NAME.replace(".bin", "") + "_"

try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
ref_mgr = prog.getReferenceManager()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def in_program(addr):
    return prog.getMemory().contains(addr)


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None or not in_program(addr):
        print("[skip] address not in program: " + addr_str)
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function at " + addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    refs = list(ref_mgr.getReferencesTo(func.getEntryPoint()))
    out_path = os.path.join(OUT_DIR, PREFIX + addr_str.lower() + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== %s %s (entry=%s) ==\n" % (
            func.getName(), addr_str.lower(), func.getEntryPoint()))
        fh.write("size=%d bytes, %d instructions, %d refs to entry\n\n" % (
            body.getNumAddresses(), len(instrs), len(refs)))
        fh.write("--- CALLERS ---\n")
        for r in refs:
            from_a = r.getFromAddress()
            from_func = fm.getFunctionContaining(from_a)
            fn = from_func.getName() if from_func else "?"
            fe = str(from_func.getEntryPoint()) if from_func else "?"
            ins = listing.getInstructionAt(from_a)
            fh.write("  from %s in %s @ %s: %s\n" % (
                from_a, fn, fe, ins.toString() if ins else "?"))
        fh.write("\n--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    print("wrote " + out_path)


for t in TARGETS:
    dump(t)

print("done")
