# @category Legaia
# @runtime Jython
#
# Round 22: 0x801F-tail of the 0897 town overlay. These are smaller helpers
# (effect emitters, sub-renderers) that are cited from previously-dumped
# field-VM and ACTOR_CTRL helpers but didn't get their own dump.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# To be filled in at runtime by reading function-coverage.py output.
# Placeholders here; actual addresses pasted before run.
TARGETS = [
    "801f0348", "801f03f0", "801f0450", "801f0740", "801f07ac",
    "801f0adc", "801f1118", "801f12d0", "801f1ed4", "801f2160",
    "801f30c4", "801f3990", "801f3c34", "801f44a0", "801f45a4",
    "801f69d8", "801f7088", "801f71e0", "801f7b88", "801f8580",
    "801f8d0c", "801fd4c0", "80202b30", "80203a50", "802046b8",
    "802059f8", "8020d05c",
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

    out_path = os.path.join(OUT_DIR, "{}_{}.txt".format(PROGRAM_PREFIX, addr_str))
    if os.path.exists(out_path):
        print("[skip-exists] {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

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
