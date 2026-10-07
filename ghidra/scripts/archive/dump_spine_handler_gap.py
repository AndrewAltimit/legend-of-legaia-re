# @category Legaia
# @runtime Jython
#
# Wave 7 arc 1b: dump the remaining un-dumped entries of the field-overlay
# system-actor handler table PTR_FUN_801f33b4 (52 entries, dispatched by
# FUN_801f159c off actor+0x50; PROT 0897 file +0x24B9C). Handler 0x23 =
# FUN_801ef014 (flag-window picker), 0x29/0x2B = RIREMITO/RULA warp
# appliers, 0x24 = tile-board walk SM. Only 0x26 (FUN_801f1fd4) had no
# dump; 0x1A (FUN_801f20b0, the picker's state-3 hand-off) is re-dumped
# alongside for the same thread.
#
# Run against the field overlay import:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0897.bin -noanalysis \
#       -postScript /scripts/dump_spine_handler_gap.py
#
# Output: /scripts/funcs/overlay_0897_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801f1fd4",  # handler 0x26 (also 0x28 sibling at 801f1fdc)
    "801f20b0",  # handler 0x1A - flag-window picker state-3 hand-off
    "801f03f0",  # handler 0x22 - sibling of the picker in the debug band
]

OUT_DIR = "/scripts/funcs"

prog = getCurrentProgram()
label = prog.getName().replace(".bin", "").replace(".BIN", "")
fm = prog.getFunctionManager()
listing = prog.getListing()
mem = prog.getMemory()
af = prog.getAddressFactory()

dec = DecompInterface()
dec.setOptions(DecompileOptions())
dec.openProgram(prog)
monitor = ConsoleTaskMonitor()


def in_program(addr):
    return mem.contains(addr)


for t in TARGETS:
    addr = af.getAddress("0x" + t)
    if addr is None or not in_program(addr):
        print("skip %s (not in %s)" % (t, label))
        continue
    fn = fm.getFunctionContaining(addr)
    if fn is None:
        ok = disassemble(addr)
        fn = createFunction(addr, "FUN_" + t)
        if fn is None:
            print("cannot create function at %s" % t)
            continue
    out = os.path.join(OUT_DIR, "overlay_%s_%s.txt" % (label, t))
    lines = []
    lines.append(
        "== spine-handler dump %s [%s] ==" % (t, prog.getName())
    )
    lines.append(
        "containing fn: %s entry=%s min=%s max=%s"
        % (fn.getName(), fn.getEntryPoint(), fn.getBody().getMinAddress(), fn.getBody().getMaxAddress())
    )
    lines.append("")
    lines.append("--- DISASSEMBLY ---")
    it = listing.getInstructions(fn.getBody(), True)
    while it.hasNext():
        insn = it.next()
        lines.append("%s  %s" % (insn.getAddress(), insn))
    lines.append("")
    lines.append("--- DECOMPILED ---")
    try:
        res = dec.decompileFunction(fn, 90, monitor)
        if res is not None and res.decompileCompleted():
            lines.append(res.getDecompiledFunction().getC())
        else:
            lines.append("(decompile failed)")
    except Exception as e:
        lines.append("(decompile exception: %s)" % e)
    f = open(out, "w")
    f.write("\n".join(lines))
    f.close()
    print("dumped %s -> %s" % (t, out))
