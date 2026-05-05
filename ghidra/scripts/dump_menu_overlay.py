# @category Legaia
# @runtime Jython
# Dump menu overlay's top 20 functions by size.

import os
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_menu_"

# Top 20 from inventory
TARGETS = [
    "801dd35c", "801d33d8", "801e1c1c", "801d6e18", "801d4c28",
    "801d1290", "801d0520", "801d21c0", "801e08d8", "801d9c14",
    "801dc1cc", "801cf88c", "801db380", "801db7f4", "801dbd94",
    "801d8308", "801e3294", "801cfd68", "801e2ee4", "801d0f1c",
]

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        return
    func = fm.getFunctionAt(addr) or fm.getFunctionContaining(addr)
    if func is None:
        print("[skip] {}".format(addr_str))
        return
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
    print("wrote " + out_path)


for t in TARGETS:
    dump(t)
print("done")
