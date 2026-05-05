# @category Legaia
# @runtime Jython
#
# Round 7 (2026-05-04 session 25): dump the top-30 most-cited helpers that
# don't yet have a function dump. Targets are split across two programs:
#
# - SCUS_942.54         : 16 helpers in the 0x80017xxx-0x8005xxxx range
# - overlay_0897_xxx_dat: 14 helpers in the 0x801CFxxx-0x801D7xxx range
#
# Run twice, once per program:
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process SCUS_942.54 \
#     -noanalysis -prescript /scripts/dump_round7_missing.py
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process overlay_0897_xxx_dat.bin \
#     -noanalysis -prescript /scripts/dump_round7_missing.py
#
# The script dispatches on the program name: SCUS dumps go to
# `<addr>.txt`, overlay dumps to `overlay_0897_<addr>.txt`.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    "80031d00", "8003cd00", "80035a4c", "8003cbf8", "80017d98",
    "8001be80", "8001d184", "8003ca78", "80059510", "80059744",
    "8005bee4", "8005c4ac", "8005d424", "8005db9c", "8005e788",
    "8005fedc",
]

OVERLAY_TARGETS = [
    "801d43ec", "801cf650", "801db7b0", "801d01b0", "801d03c4",
    "801d31b0", "801d32f8", "801d3444", "801d3748", "801d52d0",
    "801d59c8", "801d6274", "801d6b4c", "801d7b50",
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


def dump(addr_str, prefix):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip] {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    if prefix:
        out_name = "{}_{}.txt".format(prefix, addr_str)
    else:
        out_name = "{}.txt".format(addr_str)
    out_path = os.path.join(OUT_DIR, out_name)
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


print("program: {}".format(prog_name))
if "SCUS" in prog_name:
    for t in SCUS_TARGETS:
        dump(t, prefix=None)
elif "0897" in prog_name:
    for t in OVERLAY_TARGETS:
        dump(t, prefix="overlay_0897")
else:
    print("(no targets for this program; skipping)")

print("done")
