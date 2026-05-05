# @category Legaia
# @runtime Jython
#
# Round 8 (2026-05-04 session 25): chase the next batch of missing helpers
# surfaced after round 7. Includes the field-VM epilogue FUN_801DF8DC
# (now the top-cited orphan) and the rest of the 801D... cluster called
# from 0x22 / 0x23 / dispatcher tail.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    "8003cc98", "8005bd40",
]

OVERLAY_TARGETS = [
    # Top of round-7 missing list, all in 0897 overlay window
    "801df8dc",
    "801d7bb8", "801d84c0", "801d9bbc", "801d9d3c", "801da1f8",
    "801da34c", "801da59c", "801da780", "801dba04", "801dbb8c",
    "801dbc30", "801dbd04", "801dbddc", "801dbec4", "801dceac",
    "801dd0ac", "801ddc20",
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
