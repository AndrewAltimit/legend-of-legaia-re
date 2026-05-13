# @category Legaia
# @runtime Jython
#
# Dump the three SCUS functions that write to gp[0x148] (the drawable-list
# head consumed by FUN_80031D00's walker -> FUN_8002C69C continent-terrain
# emitter):
#
#   FUN_800353E0
#   FUN_800319A8
#   FUN_800326AC
#
# Output: /scripts/funcs/<addr>.txt (disassembly + decompiled C + caller list).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = ["800353e0", "800319a8", "800326ac"]

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def dump_func(addr_str):
    addr = af.getAddress(addr_str)
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for %s" % addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, addr_str + ".txt")
    refs = list(ref_mgr.getReferencesTo(addr))
    with open(out_path, "w") as fh:
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
            res = decomp.decompileFunction(func, 90, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    print("wrote %s" % out_path)


for t in TARGETS:
    dump_func(t)

print("done")
