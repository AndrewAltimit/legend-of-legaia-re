# @category Legaia
# @runtime Jython
#
# Dumps the top-20 world_map_top overlay functions by size.
# Output naming: overlay_world_map_top_<addr>.txt.
#
# overlay_world_map_top.bin is a shorter world-map capture (top-view /
# aerial camera mode, no movement) -- 129 functions, missing the main
# dispatcher FUN_801DE840 and the dev menu FUN_801EAD98 that appear in
# overlay_world_map_walk.bin.  Key functions present here:
#   FUN_801E76D4 -- world map controller (9320 bytes)
#   FUN_801DC0BC -- (4692 bytes)
#   FUN_801D6704 -- MAIN INIT (3604 bytes)
#
# Run against overlay_world_map_top.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_world_map_top.bin -noanalysis \
#       -postScript /scripts/dump_world_map_top_overlay.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801e76d4",  # 9320 bytes -- world map controller
    "801dc0bc",  # 4692 bytes
    "801d6704",  # 3604 bytes -- MAIN INIT
    "801dab90",  # 2432 bytes
    "801e5b4c",  # 2228 bytes
    "801d84d0",  # 2176 bytes (2176 in top vs 5996 in walk -- analysis delta)
    "801ed710",  # 2032 bytes -- MES renderer
    "801d01b0",  # 1964 bytes
    "801e3e00",  # 1648 bytes
    "801d0d38",  # 1548 bytes
    "801d9e1c",  # 1396 bytes
    "801e4794",  # 1220 bytes
    "801d1344",  # 1332 bytes
    "801e3984",  # 1148 bytes
    "801d31b0",  # 1148 bytes
    "801e6b34",  # 1084 bytes
    "801db510",  # 988 bytes
    "801d1ec4",  # 980 bytes
    "801e4d8c",  # 968 bytes
    "801cfe4c",  # 868 bytes
    "801ce9c4",  # 324 bytes -- unique to world_map_top (caseD_0)
]

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_world_map_top_"

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
        fh.write("== {} {} (entry={}) [overlay_world_map_top base=0x801C0000] ==\n".format(
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
