# @category Legaia
# @runtime Jython
#
# Round 5 (2026-05-04): dump the top-25 most-cited overlay helpers
# (0x801D... range) that don't yet have a function dump. Run with:
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process overlay_0897_xxx_dat.bin \
#     -noanalysis -prescript /scripts/dump_top_overlay_missing.py
#
# Output naming follows the established pattern:
#     overlay_0897_<addr>.txt
#
# Targets pre-computed via `python3 scripts/function-coverage.py --json`
# filtered to the 0x801D0000..0x801E0000 window.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d896c", "801daa50", "801db8ec", "801d5630", "801de3e0",
    "801d596c", "801d65d8", "801d8be0", "801d99bc", "801dc0bc",
    "801de004", "801de190", "801d2d38", "801d77f4", "801d79e8",
    "801d81e0", "801d8280", "801d835c", "801d8450", "801d8a88",
    "801d8d00", "801d9e1c", "801da6b4", "801dab90", "801db81c",
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
