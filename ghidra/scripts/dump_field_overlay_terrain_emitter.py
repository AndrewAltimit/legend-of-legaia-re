# @category Legaia
# @runtime Jython
#
# Dumps the field overlay (overlay_0897_xxx_dat.bin) functions caught
# writing to the world-map prim pool by autorun_lzs_and_bundle_probe.lua.
#
# The pool Write probe at 0x800B5000 (deep Buffer A inside the prim pool)
# captured 100+ writes during a town -> world-map transition. PC
# distribution clustered into two groups, both inside the 0897 field
# overlay (which extends past 0x801F0000, past the end of
# overlay_world_map.bin):
#
#   0x801F6F10..0x801F6F8C : inside FUN_801F5748 (already dumped as
#                            overlay_0897_801f5748.txt).
#   0x801F8D0C..0x801F8E2C : inside FUN_801F8D0C (already dumped as
#                            overlay_0897_801f8d4c.txt). Contains the
#                            hottest inner-loop PCs 0x801F8D74/D78.
#   0x801F8C08             : in the gap between FUN_801F8580 (ends
#                            0x801F8AB0) and FUN_801F8D0C. Undumped.
#   0x801F8E4C             : in the small gap between FUN_801F8D0C
#                            (ends 0x801F8E34) and FUN_801F8E6C (1-byte
#                            stub). Undumped.
#
# Run against overlay_0897_xxx_dat.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0897_xxx_dat.bin -noanalysis \
#       -postScript /scripts/dump_field_overlay_terrain_emitter.py
#
# Output naming: overlay_0897_<addr>.txt (matches existing 0897 dumps).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801f8c08",  # in gap between FUN_801F8580 and FUN_801F8D0C
    "801f8e4c",  # in gap between FUN_801F8D0C and FUN_801F8E6C
]

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_0897_"

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
            fh.write("(decompile exception: {})\n".format(str(e)))
    print("wrote " + out_path)


for t in TARGETS:
    dump(t)

print("done")
