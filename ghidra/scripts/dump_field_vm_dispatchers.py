# @category Legaia
# @runtime Jython
#
# Dumps the four SCUS-resident dispatchers cited in the field VM:
#   FUN_8003CE08  - high-byte 0x50 default-route (SET flag)
#   FUN_8003CE34  - high-byte 0x60 default-route (CLEAR flag, predicted)
#   FUN_8003CE64  - high-byte 0x70 default-route (TEST flag)
#   FUN_8003C5F0  - generic ramp scheduler (cited by 0x43 sub-3..6, 0x4C sub-1)
#
# Some of these aren't auto-detected as functions; we create them on the fly
# before decompiling.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "8003ce08",
    "8003ce34",
    "8003ce64",
    "8003c5f0",
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
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def ensure_function(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        return None
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is not None:
        return func
    # Create function at the address.
    try:
        from ghidra.app.cmd.function import CreateFunctionCmd
        cmd = CreateFunctionCmd(addr)
        if cmd.applyTo(prog, monitor):
            func = fm.getFunctionAt(addr)
            if func is not None:
                print("[create] {}: created function {}".format(addr_str, func.getName()))
                return func
        print("[create] {}: applyTo failed: {}".format(addr_str, cmd.getStatusMsg()))
    except Exception as e:
        print("[create] {}: exception {}".format(addr_str, e))
    return None


def dump(addr_str):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip] no function for {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, addr_str + ".txt")
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
