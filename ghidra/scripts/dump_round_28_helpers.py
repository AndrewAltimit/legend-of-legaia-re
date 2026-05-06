# @category Legaia
# @runtime Jython
#
# Dump the 7 dialog-overlay missing helpers reported by
# scripts/function-coverage.py. One target lives in SCUS_942.54
# (80032434), the other six are in the overlay_dialog_mc4 program.
#
# Run twice with -process: once with SCUS_942.54 and once with
# overlay_dialog_mc4. Each invocation only dumps targets whose entry
# point lies in that program's memory.
#
# Output filename follows the existing conventions:
#   - SCUS dumps: /scripts/funcs/<addr>.txt
#   - overlay_dialog_mc4 dumps: /scripts/funcs/overlay_dialog_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "80032434",  # cited from overlay_dialog_801ecd0c (SCUS)
    "801cf754",  # cited from overlay_dialog_801d1344
    "801d0b90",  # cited from overlay_dialog_801d1344
    "801d1ba0",  # cited from overlay_dialog_801d1344
    "801d9d30",  # cited from overlay_dialog_801d1344
    "801db510",  # cited from overlay_dialog_801d1344
    "801de234",  # cited from overlay_dialog_801d1344
]

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    if prog_name.startswith("overlay_dialog"):
        return os.path.join(OUT_DIR, "overlay_dialog_" + addr_str + ".txt")
    # Generic fallback - derive a stable label from program name.
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    block = mem.getBlock(addr)
    return block is not None


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        # Address isn't in this program's memory - leave for the other run.
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
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
    finally:
        fh.close()
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done [{}]".format(prog_name))
