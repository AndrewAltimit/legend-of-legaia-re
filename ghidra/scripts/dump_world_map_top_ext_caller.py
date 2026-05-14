# @category Legaia
# @runtime Jython
#
# Dumps the function at 0x801F7088 in the world_map_top_ext overlay -
# the function containing the JAL to FUN_80043390 at PC 0x801F78CC
# whose return address (0x801F78D4) matches the captured caller-RA from
# the Drake slot-4 Read-bp probe. This is the overlay-side caller that
# drives cluster A during the warp-into-world-map transition.
#
# Also dumps PC 0x801F8968 + nearby kind handlers as a safety net.
#
# Run against overlay_world_map_top_ext.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_world_map_top_ext.bin \
#       -postScript /scripts/dump_world_map_top_ext_caller.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.app.cmd.function import CreateFunctionCmd

TARGETS = [
    ("801f7088", "wm_ext_dispatcher_caller"),  # contains JAL to FUN_80043390 at 0x801F78CC
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


def ensure_function(addr):
    func = fm.getFunctionContaining(addr)
    if func is not None:
        return func
    func = fm.getFunctionAt(addr)
    if func is not None:
        return func
    cmd = CreateFunctionCmd(addr)
    if cmd.applyTo(prog, monitor):
        return fm.getFunctionAt(addr)
    return None


def dump(addr_str, label):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] not an address: " + addr_str)
        return
    func = ensure_function(addr)
    if func is None:
        print("[fail] could not create function at " + addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR,
        "overlay_world_map_top_ext_" + label + "_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== %s %s (entry=%s, label=%s) [world_map_top_ext.bin] ==\n" % (
            func.getName(), addr_str, func.getEntryPoint(), label))
        fh.write("size=%d bytes, %d instructions\n\n" % (
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % str(e))
    print("wrote %s (%d bytes)" % (out_path, body.getNumAddresses()))


for addr_str, label in TARGETS:
    dump(addr_str, label)

print("done")
