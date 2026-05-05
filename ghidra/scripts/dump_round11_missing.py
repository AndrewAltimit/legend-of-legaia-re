# @category Legaia
# @runtime Jython
#
# Round 11 (session 28): focus on battle subsystem -- archive loader
# children (FUN_80052FA0 + FUN_800542C8 cluster) plus a few
# direct children of the battle state machine FUN_801E295C and the
# inventory FUN_80042558 cluster surfaced last session.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    # Battle archive loader children (FUN_80052FA0 = battle archive loader):
    "800536bc", "80053898", "80053b9c", "80053cb8", "800557b8", "80055854",
    # FUN_800542C8 = secondary battle archive loader:
    "80054cb0", "80055468",
    # Battle state-machine SCUS-side children (cited from overlay 0898 0x801E295C):
    "8004e2f0", "80050e2c", "80055b4c",
    # Inventory parent + children (FUN_80042558 cluster -- 80042DBC was already done in round 10):
    "80042558", "800431fc", "80043264", "800432bc",
    # Renderer children + actor cleanup chain:
    "80029724", "80026f50", "80034a6c", "800337b0",
    # Streaming consumers helpers (cited from streaming_consumers.py outputs):
    "800513f0", "80051d84",
    # Misc 2-cite helpers:
    "8005b268", "8005b308", "8005b818",
    # Top-cited 1-ref helpers from text-renderer / 80030628:
    "80030104", "800302e4", "8003043c", "8003053c", "8002ff8c",
    # Inventory page-bank cluster (FUN_800480d8 / FUN_800495c8 / 80049858 / 8004998c):
    "800480d8", "800495c8", "80049858", "8004998c",
    # Mode-init child (cited from 8001c394 -- per-stage init):
    "800460ac",
    # SCUS helpers cited from battle overlay (live in SCUS_942.54 at these
    # addresses; the citations are from overlay_0898_801d8de8 and
    # overlay_0898_801ec3e4 which jal back into SCUS):
    "8003cac4", "8004fe5c",
]

OVERLAY_0898_TARGETS = [
    "8003cac4",   # cited from 801D8DE8 (hottest battle utility)
    "8004fe5c",   # cited from 801EC3E4
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
elif "overlay.bin" == prog_name:
    # The disc-extracted battle overlay (0898) was imported under the
    # generic name `overlay.bin` (base 0x801CE818).
    for t in OVERLAY_0898_TARGETS:
        dump(t, prefix="overlay_0898")
else:
    print("(no targets for this program; skipping)")

print("done")
