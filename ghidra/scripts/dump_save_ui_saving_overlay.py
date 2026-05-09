# @category Legaia
# @runtime Jython
#
# Dumps the top-20 save_ui_saving overlay functions by size.
# Output naming: overlay_save_ui_saving_<addr>.txt.
#
# overlay_save_ui_saving.bin (129 functions) was captured during an active
# save operation (after slot selection, while writing). Its function layout
# is identical to overlay_save_ui_select.bin at the same addresses and sizes,
# confirming that the save/load screen is a single resident overlay with
# sub-state routing rather than two distinct overlays.
#
# Run against overlay_save_ui_saving.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_save_ui_saving.bin -noanalysis \
#       -postScript /scripts/dump_save_ui_saving_overlay.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801dd35c",  # 12104 bytes -- save UI main frame / render loop
    "801d33d8",  # 5264 bytes
    "801e1c1c",  # 4520 bytes  -- menu main dispatcher
    "801d6e18",  # 3560 bytes  -- wait / fade handler
    "801d4c28",  # 2280 bytes
    "801d1290",  # 2192 bytes
    "801d0520",  # 2040 bytes
    "801d21c0",  # 1872 bytes
    "801e08d8",  # 1784 bytes
    "801d9c14",  # 1676 bytes
    "801dc1cc",  # 1256 bytes
    "801cf88c",  # 1244 bytes
    "801db380",  # 1140 bytes
    "801db7f4",  # 1128 bytes
    "801dbd94",  # 1080 bytes
    "801d8308",  # 1068 bytes
    "801e3294",  # 1036 bytes
    "801cfd68",  # 992 bytes
    "801d6628",  # 612 bytes   -- actor VM entry (FUN_801D6628)
    "801d688c",  # 456 bytes
]

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_save_ui_saving_"

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
        fh.write("== {} {} (entry={}) [overlay_save_ui_saving base=0x801C0000] ==\n".format(
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
