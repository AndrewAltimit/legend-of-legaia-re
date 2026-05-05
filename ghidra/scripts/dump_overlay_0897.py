# @category Legaia
# @runtime Jython
#
# Dump 0897_xxx_dat (TOWN/FIELD/MES overlay) functions of interest. This
# is the largest overlay yet -- 318 KB / 383 functions. Confirmed as the
# town overlay by byte-delta match against /tmp/legaia_overlay_town.bin
# (delta 0xE818). Strings include "meswork music_not_change", "PX%d PY%d",
# "map read", "vdf n %x", "SUB CMD ERROR", "Cannot equip", "Hyper Arts",
# "Magic effect: ...". Hosts: field VM, MES dialog renderer, inventory
# system, battle UI shadow.
#
# Address range 0x801CE818 - 0x80218018 (loaded base 0x801CE818, file 0x4F800).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801de840",  # 17540 bytes / 357 outgoing -- biggest function ever found
    "801f5748",  # 11108 bytes / 192 outgoing -- second dispatcher
    "801d6704",  # 2820 bytes / 78 outgoing -- previously-known MAIN INIT thunk target
    "801ead98",  # 5968 bytes / 35 calls / 1 in -- subsystem entry
    "801d362c",  # 4524 bytes / 60 outgoing -- mid-tier dispatcher
    "801ed710",  # 1904 bytes / 44 outgoing / 3 in -- possible MES renderer
    "801ef2b0",  # 1920 bytes / 29 outgoing
    "801fdde8",  # 2248 bytes / 25 outgoing
    "801d4a60",  # 2760 bytes / 37 outgoing
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

    out_path = os.path.join(OUT_DIR, "overlay_0897_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_0897 base=0x801CE818] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint()))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 300, monitor)
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
