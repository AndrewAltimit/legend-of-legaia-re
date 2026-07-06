# @category Legaia
# @runtime Jython
#
# Spine flag-writer content grep (wave 7 arc 1b). Decompiles every function
# in the current program and dumps the C for any whose body references the
# spine-writer targets: the flag setters (FUN_8003CE08 / CE34), the flag
# bank DAT_80085758.., the Zeto battle-id global _DAT_8007b7fc, or the FMV
# id global _DAT_8007ba78. The decompiler resolves computed/pointer-based
# stores that instruction-level LUI trackers miss.
#
# Run against any program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process <program> -noanalysis \
#       -postScript /scripts/dump_spine_refs.py
#
# Output: /scripts/funcs/spine_refs_<program>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

NEEDLES = [
    "8003ce08",
    "8003ce34",
    "8007b7fc",
    "8007ba78",
    "8008575",
    "8008576",
    "8008577",
    "8008578",
    "8008579",
    "0x142",
    "0x482",
]

prog = getCurrentProgram()
pname = prog.getName().replace(".bin", "").replace(".BIN", "").replace(".", "_")
OUT = "/scripts/funcs/spine_refs_%s.txt" % pname

dec = DecompInterface()
dec.setOptions(DecompileOptions())
dec.openProgram(prog)
monitor = ConsoleTaskMonitor()

lines = []
fns = prog.getFunctionManager().getFunctions(True)
count = 0
hit = 0
for fn in fns:
    count += 1
    try:
        res = dec.decompileFunction(fn, 60, monitor)
        if res is None or not res.decompileCompleted():
            continue
        c = res.getDecompiledFunction().getC()
    except Exception:
        continue
    lc = c.lower()
    matched = [n for n in NEEDLES if n in lc]
    if matched:
        hit += 1
        ent = fn.getEntryPoint()
        lines.append("===== %s @ %s  needles: %s =====" % (fn.getName(), ent, ",".join(matched)))
        lines.append(c)
        lines.append("")

with open(OUT, "w") as f:
    f.write("# %s functions referencing spine flag-writer targets\n" % prog.getName())
    f.write("# needles: %s\n" % ", ".join(NEEDLES))
    f.write("# scanned %d functions, %d hits\n\n" % (count, hit))
    f.write("\n".join(lines))

print("spine_refs: scanned %d functions, %d hits -> %s" % (count, hit, OUT))
