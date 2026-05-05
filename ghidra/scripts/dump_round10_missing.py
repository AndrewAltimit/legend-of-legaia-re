# @category Legaia
# @runtime Jython
#
# Round 10 (session 27): cover the move-VM downstream cluster (80024C80,
# 80024DFC, 80024EE4, 800250D4, 80019D50) and the text-actor tick
# children (80031D00 cluster) that surface as the next-tier missing
# helpers after round 9. Also picks up two repeat-cited overlay-0897
# helpers (801D2CFC, 801DA0F0).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    # Move-VM and actor-tick downstream:
    "80019d50", "8001ffa4", "80024c80", "80024dfc", "80024ee4",
    "800250d4", "80026410", "800267fc", "80026be0", "80026c18",
    "80026ce4", "800271a8", "80027f00", "8002a5a4",
    # 8002b3d0+ helper cluster (referenced from 8001e1b4, 80017888,
    # 80017b94, 80016444, 8001822c, 8001ada4):
    "8002b3d4", "8002b468", "8002b584", "8002b688", "8002b790",
    "8002b93c", "8002b944", "8002daa4",
    # Text-actor-tick (80031D00) children:
    "80031ae4", "80034250", "80034358", "80035cb8", "80035da0",
    "80035e44", "8003cc88",
    # 8003AEB0 cluster (and friends):
    "8003a024", "8003a110", "8003a55c", "8003a9d4", "8003ab2c",
    "800353e0", "8003c110", "8003cd68", "8003d190",
    # Misc 1-cite high-value helpers:
    "8005860c", "80042dbc", "80037044", "800379a8", "8003774c",
    "8003ee00", "8003e7f0", "8003e104",
]

OVERLAY_TARGETS = [
    "801d2cfc", "801da0f0",
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
