# @category Legaia
# @runtime Jython
#
# Dump 0898_xxx_dat (Battle overlay) functions of interest. Re-imported
# at base 0x801CE818 -- the disc-extracted file matches the previously
# captured battle save state with constant offset delta 0xE818, confirming
# that PROT entry 873/0898 IS the battle overlay loaded by FUN_800520f0.
# Address range: 0x801CE818 - 0x801F8018.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801e295c",  # 16396 bytes / 155 outgoing -- biggest dispatcher
    "801d0748",  # 11124 bytes / 182 outgoing -- second-biggest dispatcher
    "801ec3e4",  # 10008 bytes / 17 outgoing
    "801e9fd4",  # 8456  bytes / 46 outgoing / 1 in
    "801d388c",  # 7820  bytes / 83 outgoing / 39 in -- battle helper hub
    "801d5854",  # 6500  bytes / 9 outgoing / 47 in -- hot utility
    "801d8de8",  # 3028  bytes / 23 outgoing / 77 in -- extremely hot
    "801dfdf8",  # known effect-bundle spawn API (from save-state capture)
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

    out_path = os.path.join(OUT_DIR, "overlay_0898_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_0898 base=0x801CE818] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint()))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 240, monitor)
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
