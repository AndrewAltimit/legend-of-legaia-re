# @category Legaia
# @runtime Jython
#
# Round 6 (2026-05-04): dump the next-25 most-cited overlay helpers
# (0x801D... / 0x801E... / 0x801F... range) that don't yet have a function
# dump. Run with:
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process overlay_0897_xxx_dat.bin \
#     -noanalysis -prescript /scripts/dump_round6_overlay_missing.py
#
# Output naming follows the established pattern:
#     overlay_0897_<addr>.txt
#
# Targets pre-computed via `python3 scripts/function-coverage.py --json`
# filtered to the overlay window (0x801D0000+) on 2026-05-04.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d2f38", "801d32bc", "801d5718", "801d57e8", "801d88cc",
    "801db8b4", "801db8f4", "801dbc20", "801dd310", "801dd9d4",
    "801dde34", "801ddf48", "801ddfe4", "801de084", "801de698",
    "801dfdf0", "801e3620", "801e4c58", "801e573c", "801e57f0",
    "801f8004", "801f88fc", "801f8d4c", "801f8e6c", "801f8f28",
]

PROGRAM_PREFIX = "overlay_0897"

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
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def ensure_function(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        return None
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is not None:
        return func
    try:
        from ghidra.app.cmd.function import CreateFunctionCmd
        cmd = CreateFunctionCmd(addr)
        if cmd.applyTo(prog, monitor):
            func = fm.getFunctionAt(addr)
            if func is not None:
                print("[create] {}: {}".format(addr_str, func.getName()))
                return func
        print("[create-fail] {}: {}".format(addr_str, cmd.getStatusMsg()))
    except Exception as e:
        print("[create-exc] {}: {}".format(addr_str, e))
    return None


def dump(addr_str):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip] {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, "{}_{}.txt".format(PROGRAM_PREFIX, addr_str))
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
            fh.write("(decompile exception: {})\n".format(e))
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done")
