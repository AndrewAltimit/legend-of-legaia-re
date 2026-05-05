# @category Legaia
# @runtime Jython
#
# Dump 0978_other_game (FIELD overlay) functions of interest. Strings at
# the head of the file: "f_read %d size %d KB", "data\field\player.lzs",
# "FIELD BACK READ NOW", "FIELD BACK READ COMPLETE", "efect init",
# "battle bgm %d", "brule.xxx" -- this is the field-mode overlay.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801c82dc",  # giant dispatcher (2088 bytes, 17 outgoing calls)
    "801c39b8",  # mid-tier (916 bytes, 13 outgoing calls)
    "801c8b04",  # mid-tier (520 bytes, 12 outgoing calls)
    "801c2b58",  # mid-tier (1196 bytes, 10 outgoing calls)
    "801c3004",  # 1288 bytes, 8 outgoing calls
    "801c8d0c",  # 1260 bytes, 8 outgoing calls
    "801c5c58",  # 1476 bytes, 4 outgoing calls -- big and quiet (data ref?)
    "801c7b40",  # 1224 bytes, 5 outgoing calls
]

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        func = fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, "overlay_0978_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_0978] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint()))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 90, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done")
