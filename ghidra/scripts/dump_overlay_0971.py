# @category Legaia
# @runtime Jython
#
# Dump 0971 overlay (debug-menu / dev-omnibus) functions of interest.
# Strings at the head of the file: "DEBUG MODE", "FOG type %d", "TMD NO %d",
# "1 FISH", "2 TEST", "3 MUSIC", "4 PACHI", "5 BOKO", "6 MINI BATTLE",
# "7 DANCE", "OTHER MODE", "OMAKE DEBUG", "MOTOR0", etc. Run with:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0971.bin -noanalysis \
#       -postScript /scripts/dump_overlay_0971.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801c0164",  # entry / probable per-frame loop (2492 bytes, 29 outgoing calls)
    "801c56b4",  # giant dispatcher (5864 bytes, 42 outgoing calls)
    "801c7930",  # third-largest dispatcher (2384 bytes, 28 outgoing calls)
    "801c3f44",  # mid-tier dispatcher (1172 bytes, 24 outgoing calls)
    "801c97a4",  # mid-tier dispatcher (1024 bytes, 22 outgoing calls)
    "801c6fec",  # 1708 bytes, 12 outgoing calls
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

    out_path = os.path.join(OUT_DIR, "overlay_0971_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_0971] ==\n".format(
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
