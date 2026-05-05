# @category Legaia
# @runtime Jython
#
# Round 21: 0x801E-heavy batch (the field VM dispatcher's own immediate
# helpers cluster here). Some of these may already be inside larger
# enclosing functions; the ensure_function path handles that.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d828c", "801da8bc", "801da8f0", "801db2c0", "801dbf7c",
    "801de468", "801df570", "801e22c4", "801e23ec", "801e2640",
    "801e2650", "801e3578", "801e3658", "801e3764", "801e3894",
    "801e3984", "801e3e00", "801e4404", "801e4420", "801e45ac",
    "801e4c38", "801e5520", "801e565c", "801e5b4c", "801e5e84",
    "801e5fb0", "801e60a8", "801e6388", "801e63e0", "801e66d8",
    "801e6968", "801e6d84", "801e70bc", "801e7250", "801e7320",
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
