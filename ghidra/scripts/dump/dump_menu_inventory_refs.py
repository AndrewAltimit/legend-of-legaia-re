# @category Legaia
# @runtime Jython
#
# Decompiles every function in the current program (run against
# overlay_menu.bin) and dumps the C for any whose body touches the item
# inventory: either the array itself (0x80085958/_59) or the shared SCUS
# accessor family (FUN_80042310/_42EE0/_42F4C/_423E0/_43048/_4313C) and the
# gp-relative window registers (gp+0x2D2/0x2D4/0x2D6). Robust against the
# LUI+ADDIU xref gap: matches on the decompiled-C text, not the reference
# manager. Used to audit whether the item menu writes the inventory by raw
# index (it does not -- every mutation goes through the bounds-checked
# helpers). Output: /scripts/funcs/menu_inventory_ops.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

NEEDLES = ["80042310", "80042ee0", "80042f4c", "800423e0", "80043048", "8004313c", "0x2d4)", "0x2d2)", "0x2d6)"]
OUT = "/scripts/funcs/menu_inventory_ops.txt"

prog = getCurrentProgram()
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
    if any(n in c for n in NEEDLES):
        hit += 1
        ent = fn.getEntryPoint()
        lines.append("===== %s @ %s =====" % (fn.getName(), ent))
        lines.append(c)
        lines.append("")

with open(OUT, "w") as f:
    f.write("# menu-overlay functions referencing the inventory array 0x80085958\n")
    f.write("# scanned %d functions, %d hits\n\n" % (count, hit))
    f.write("\n".join(lines))

print("scanned %d functions, %d reference the inventory; wrote %s" % (count, hit, OUT))
