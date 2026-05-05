# @category Legaia
# @runtime Jython
#
# Round 20: continuation of round 19. Next 35 missing field-VM callees,
# now extending into the 0x801E address range (closer to FUN_801DE840
# itself).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d0290", "801d567c", "801d5c58", "801d701c", "801d71f0",
    "801d7334", "801d7e14", "801d9c0c", "801daba4", "801db318",
    "801db9c4", "801dbf9c", "801dc098", "801dc0a0", "801dc188",
    "801dd0c0", "801dd260", "801dd8f0", "801dddec", "801de37c",
    "801de478", "801de4c8", "801de604", "801deab4", "801df6b8",
    "801e0080", "801e0b1c", "801e0f3c", "801e0f40", "801e1538",
    "801e158c", "801e175c", "801e1c64", "801e22c8", "801e2524",
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
