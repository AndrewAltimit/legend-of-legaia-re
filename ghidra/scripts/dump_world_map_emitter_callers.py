# @category Legaia
# @runtime Jython
#
# Dump the six world-map-overlay functions that JAL directly to
# FUN_8002C69C (the POLY_FT4/SPRT emitter). These DON'T go through the
# gp[0x148] drawable-list walker (FUN_80031D00) - they call the emitter
# inline, setting gp[0x14C] = mode_byte in the call-site preamble.
#
# Targets discovered by find_jal_target.py against
# overlay_world_map.bin / overlay_world_map_top.bin /
# overlay_world_map_walk.bin (same 7-8 hits across all three).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)

PROGRAM_NAME = prog.getName()
OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass


def out_path_for(addr_str):
    if "overlay" in PROGRAM_NAME.lower():
        label = PROGRAM_NAME.replace(".bin", "")
        return os.path.join(OUT_DIR, "%s_%s.txt" % (label, addr_str))
    return os.path.join(OUT_DIR, "%s.txt" % addr_str)


def dump_func_at(addr_int):
    addr_str = "%08x" % addr_int
    addr = af.getAddress(addr_str)
    if addr is None:
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out = out_path_for(addr_str)
    refs = list(ref_mgr.getReferencesTo(addr))
    with open(out, "w") as fh:
        fh.write("== %s %s (entry=%s) ==\n" % (
            func.getName(), addr_str, func.getEntryPoint()))
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
            res = decomp.decompileFunction(func, 120, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    print("wrote %s" % out)


# The 6 world-map overlay functions that JAL directly to FUN_8002C69C.
TARGETS = [
    0x801D2EBC,  # 1 call - first occurrence
    0x801D84D0,  # 2 calls - largest cluster, likely main continent loop
    0x801DA7F0,  # 1 call
    0x801E6984,  # 1 call
    0x801E6B34,  # 1 call
    0x801EAD98,  # 1 call - in non-top/walk only, dev menu renderer
    0x801EF2B0,  # 1 call
]

for a in TARGETS:
    dump_func_at(a)

print("done")
