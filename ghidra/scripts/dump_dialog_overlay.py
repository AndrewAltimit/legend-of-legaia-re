# @category Legaia
# @runtime Jython
#
# Dumps the top dialog-overlay candidates for the MES bytecode dispatcher
# search. Output naming: overlay_dialog_<addr>.txt.
#
# Usage:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_dialog_mc4.bin -noanalysis \
#       -postScript /scripts/dump_dialog_overlay.py
#
# Background: mc4 mednafen save state captures an in-dialog NPC interaction.
# The MES bytecode dispatcher (Op4c, Op65, Op26 sub-opcodes) is overlay-
# resident and not in the 0897 town overlay; this script targets the
# largest candidates identified via inventory_overlay.py.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor


# Top-15 dialog-overlay functions by size (from inventory CSV).
# FUN_801de840 is the field VM (already dumped from 0897). Skip it.
TARGETS = [
    "801e76d4",  # 9320 bytes -- prime MES dispatcher candidate
    "801ead98",  # 7280 bytes
    "801d84d0",  # 5996 bytes
    "801dc0bc",  # 4692 bytes
    "801d6704",  # 3604 bytes -- main_init (already dumped from 0897, dump dialog-overlay variant for diff)
    "801ef2b0",  # 3408 bytes
    "801d4a60",  # 3024 bytes
    "801e9f64",  # 2636 bytes
    "801dab90",  # 2432 bytes
    "801e5b4c",  # 2228 bytes
    "801d01b0",  # 1964 bytes
    "801ed710",  # 2032 bytes (records-screen renderer per existing 0897 dump)
    "801ecd0c",  # 1532 bytes
    "801d27e0",  # 1368 bytes
    "801d1344",  # 1332 bytes
]

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_dialog_"

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
        print("[skip] not an address: " + addr_str)
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        func = fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for " + addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_dialog_mc4 base=0x801C0000] ==\n".format(
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
