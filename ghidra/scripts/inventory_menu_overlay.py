# @category Legaia
# @runtime Jython
# Inventory the menu overlay's functions: list with size + outgoing calls
# + name. Compare to 0897 (town overlay) to find menu-specific code.

import os

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()

OUT_PATH = "/scripts/funcs/overlay_menu_inventory.txt"

funcs = []
for f in fm.getFunctions(True):
    body = f.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_calls = sum(1 for i in instrs if i.getMnemonicString().lower() == "jal")
    in_refs = sum(1 for _ in prog.getReferenceManager().getReferencesTo(f.getEntryPoint()))
    funcs.append((f.getEntryPoint().getOffset(), f.getName(),
                  body.getNumAddresses(), len(instrs), out_calls, in_refs))

funcs.sort(key=lambda x: -x[2])

with open(OUT_PATH, "w") as fh:
    fh.write("== menu overlay function inventory ==\n")
    fh.write("Source: /tmp/legaia_overlay_menu.bin (mc5 = item/magic/equip menu)\n")
    fh.write("Total: {} functions\n\n".format(len(funcs)))
    fh.write("addr      name                          size   insns   out  in\n")
    for entry, name, size, insns, out, refs in funcs:
        fh.write("{:08x}  {:<28}  {:>5}  {:>5}  {:>3}  {:>3}\n".format(
            entry, name, size, insns, out, refs))
print("wrote {}".format(OUT_PATH))
print("function count: {}".format(len(funcs)))
