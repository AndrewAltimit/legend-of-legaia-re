# @category Legaia
# @runtime Jython
# Inventory + dump top 20 of battle_action overlay.

import os
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_battle_action_"

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)

funcs = []
for f in fm.getFunctions(True):
    body = f.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_calls = sum(1 for i in instrs if i.getMnemonicString().lower() == "jal")
    in_refs = sum(1 for _ in prog.getReferenceManager().getReferencesTo(f.getEntryPoint()))
    funcs.append((f.getEntryPoint().getOffset(), f.getName(),
                  body.getNumAddresses(), len(instrs), out_calls, in_refs))

funcs.sort(key=lambda x: -x[2])

idx_path = os.path.join(OUT_DIR, "overlay_battle_action_inventory.txt")
with open(idx_path, "w") as fh:
    fh.write("== battle_action overlay function inventory ==\n")
    fh.write("Source: /tmp/legaia_overlay_battle_action.bin (mc8 = action menu open in battle)\n")
    fh.write("Total: {} functions\n\n".format(len(funcs)))
    fh.write("addr      name                          size   insns   out  in\n")
    for entry, name, size, insns, out, refs in funcs:
        fh.write("{:08x}  {:<28}  {:>5}  {:>5}  {:>3}  {:>3}\n".format(
            entry, name, size, insns, out, refs))
print("wrote {}".format(idx_path))


def dump_func(func, addr_str):
    out_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    if os.path.exists(out_path):
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(
            func.getName(), addr_str, func.getEntryPoint()))
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
    print("dumped " + out_path)


# Dump top 20 by size
for entry, _, _, _, _, _ in funcs[:20]:
    addr_str = "%08x" % entry
    func = fm.getFunctionAt(af.getAddress(addr_str))
    if func is not None:
        dump_func(func, addr_str)
print("done")
