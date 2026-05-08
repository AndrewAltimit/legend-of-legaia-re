# @category Legaia
# @runtime Jython
#
# Dumps the top-20 world_map_walk overlay functions by size.
# Output naming: overlay_world_map_walk_<addr>.txt.
#
# overlay_world_map_walk.bin is the full walking world-map capture --
# 128 functions including the main dispatcher and dev menu that are absent
# from overlay_world_map_top.bin.  Key functions:
#   FUN_801DE840 -- main world map / field dispatcher (19992 bytes)
#   FUN_801E76D4 -- world map controller (9320 bytes)
#   FUN_801EAD98 -- dev menu renderer (7280 bytes)
#   FUN_801D6704 -- MAIN INIT (3604 bytes)
#
# Run against overlay_world_map_walk.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_world_map_walk.bin -noanalysis \
#       -postScript /scripts/dump_world_map_walk_overlay.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801de840",  # 19992 bytes -- main world map / field dispatcher
    "801e76d4",  # 9320 bytes  -- world map controller
    "801ead98",  # 7280 bytes  -- dev menu renderer
    "801d84d0",  # 5996 bytes
    "801d362c",  # 5172 bytes
    "801dc0bc",  # 4692 bytes
    "801d6704",  # 3604 bytes  -- MAIN INIT
    "801ef2b0",  # 3408 bytes
    "801d4a60",  # 3024 bytes
    "801e9f64",  # 2636 bytes
    "801dab90",  # 2432 bytes
    "801e5b4c",  # 2228 bytes
    "801ed710",  # 2032 bytes  -- MES renderer
    "801d01b0",  # 1964 bytes
    "801e3e00",  # 1648 bytes
    "801d0d38",  # 1548 bytes
    "801ecd0c",  # 1532 bytes
    "801d9e1c",  # 1396 bytes
    "801d27e0",  # 1368 bytes
    "801d1344",  # 1332 bytes
]

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_world_map_walk_"

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
        fh.write("== {} {} (entry={}) [overlay_world_map_walk base=0x801C0000] ==\n".format(
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
            fh.write("(decompile exception: {})\n".format(str(e)))
    print("wrote " + out_path)


for t in TARGETS:
    dump(t)

print("done")
