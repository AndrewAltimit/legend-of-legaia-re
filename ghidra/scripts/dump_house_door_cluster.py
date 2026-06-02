# @category Legaia
# @runtime Jython
#
# Decompile the intra-town (house / interior) door cluster in the 0897 field
# overlay. A runtime write-watchpoint pinned the player reposition to
# FUN_801d01b0 (locomotion) + FUN_801d1878 (forward look-ahead) calling
# FUN_801d2404, reached from FUN_801d16fc (the per-frame player update that calls
# the locomotion controller). This dumps those callers + the projected-position
# event/door checker so the door-record (trigger tile -> target tile) format can
# be read.
#
# Run against the field overlay:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0897.bin.0 -noanalysis \
#       -postScript /scripts/dump_house_door_cluster.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = ["801d16fc", "801d2404", "801d2298", "801d1878", "801cfc40"]
RAW_WINDOWS = [
    ("801d1600", "801d1880"),  # the FUN_801d16fc caller body
    ("801d2298", "801d2600"),  # FUN_801d2298 / FUN_801d2404 region
]

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
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def addr(s):
    return af.getAddress(s)


def in_program(a):
    return mem.getBlock(a) is not None


for t in TARGETS:
    a = addr(t)
    if not in_program(a):
        print("[skip] %s not in %s" % (t, prog_name))
        continue
    fn = fm.getFunctionContaining(a)
    out = os.path.join(OUT_DIR, "overlay_0897_door_%s.txt" % t)
    with open(out, "w") as f:
        if fn is None:
            f.write("== no function containing %s in %s ==\n" % (t, prog_name))
        else:
            body = fn.getBody()
            f.write("== %s %s (entry=%s) [%s] ==\n" % (
                fn.getName(), t, fn.getEntryPoint(), prog_name))
            f.write("size=%d bytes\n\n--- DISASSEMBLY ---\n" % body.getNumAddresses())
            ins = listing.getInstructions(body, True)
            for i in ins:
                f.write("%s  %s\n" % (i.getAddress(), i.toString()))
            f.write("\n--- DECOMPILED ---\n")
            res = decomp.decompileFunction(fn, 60, monitor)
            if res and res.getDecompiledFunction():
                f.write(res.getDecompiledFunction().getC())
            else:
                f.write("<decompile failed>\n")
    print("[ok] wrote %s" % out)

for (s, e) in RAW_WINDOWS:
    sa, ea = addr(s), addr(e)
    if not in_program(sa):
        continue
    out = os.path.join(OUT_DIR, "overlay_0897_door_raw_%s_%s.txt" % (s, e))
    with open(out, "w") as f:
        f.write("== raw %s..%s [%s] ==\n" % (s, e, prog_name))
        cur = sa
        while cur.getOffset() < ea.getOffset():
            ins = listing.getInstructionAt(cur)
            if ins is None:
                f.write("%s  <no instruction>\n" % cur)
                cur = cur.add(4)
                continue
            f.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
            cur = ins.getAddress().add(ins.getLength())
    print("[ok] wrote %s" % out)

print("done")
