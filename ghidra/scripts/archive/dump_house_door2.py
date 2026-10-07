# @category Legaia
# @runtime Jython
import os
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
TARGETS = ["801cf754", "801d0d38", "801d0b90"]
OUT_DIR = "/scripts/funcs"
prog = currentProgram; fm = prog.getFunctionManager(); listing = prog.getListing()
af = prog.getAddressFactory(); mem = prog.getMemory(); monitor = ConsoleTaskMonitor()
decomp = DecompInterface(); decomp.setOptions(DecompileOptions()); decomp.openProgram(prog)
for t in TARGETS:
    a = af.getAddress(t)
    if mem.getBlock(a) is None: print("[skip] "+t); continue
    fn = fm.getFunctionContaining(a)
    out = os.path.join(OUT_DIR, "overlay_0897_door2_%s.txt" % t)
    f = open(out, "w")
    if fn is None:
        f.write("no fn @ "+t+"\n")
    else:
        f.write("== %s %s (entry=%s) ==\n" % (fn.getName(), t, fn.getEntryPoint()))
        res = decomp.decompileFunction(fn, 60, monitor)
        if res and res.getDecompiledFunction():
            f.write(res.getDecompiledFunction().getC())
    f.close(); print("[ok] "+out)
print("done")
