# @category Legaia
# @runtime Jython
#
# Round 12: continue chipping away at the function-coverage punchlist.
# Picks are the top-cited 2-ref helpers, plus high-leverage 1-cites from
# the text-renderer / inventory / mode-init / battle-archive clusters,
# plus several "sister" helpers cited from already-dumped callers (so a
# sub-op or a control-flow path can be fully traced after this dump).
#
# Strategy: prefer helpers cited from MULTIPLE places, OR helpers cited
# from a caller we already understand (so the new dump immediately
# resolves a `func_0x...` placeholder in an existing trace).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    # 2-cite helpers (highest leverage):
    "8003fb10",  # cited from 8003043c, 8003053c (text-renderer)
    "8005b038",  # cited from 8001c604, 800495c8 (mode init + inventory page-bank)
    "8005b4b8",  # cited from 800172c0, 80026f50 (renderer cleanup)

    # Allocator-ish cluster cited from 8001ada4:
    "8002b94c",
    "8002b954",
    "8002b974",  # cited from 80026ce4

    # Text/dialog cluster:
    "8003c310",  # cited from 80034358
    "8003cc90",  # cited from 80030104
    "8003f348",  # cited from 80026ce4

    # Streaming-asset consumer helper:
    "8001fe70",  # cited from 800513f0
    "80053a28",  # cited from 800513f0

    # Inventory page-bank cluster (parent: 800480d8 / 800495c8 already dumped):
    "80049348",
    "8004a908",
    "8005112c",
    "8005133c",  # cited from 800402f4 (inventory page draw)
    "80046870",  # cited from 800402f4
    "80035c00",  # cited from 800402f4

    # Per-stage init helpers cited from the mode dispatcher (8001d7f8 / 8001daf8 / 8001dcf8):
    "80056738",  # cited from 8001d7f8
    "800567b8",  # cited from 8001a068
    "8005724c",  # cited from 8001daf8
    "8005731c",  # cited from 8001daf8
    "8005afb0",  # cited from 8001d424
    "8005fe18",  # cited from 8001d424
    "8005acf8",  # cited from 8001dcf8
    "8005ad54",  # cited from 8001dcf8
    "8005b678",  # cited from 8001dcf8

    # Sister helpers cited from 8005e788 (a dumped 8005xxxx parent):
    "8005bd50",
    "8005bd70",
    "8005bdec",
    "8005e574",
    "8005beac",  # cited from 8005ea84

    # Sister helpers cited from 8005dbb4:
    "8005de80",
    "8005dea0",
    "8005e180",
    "8005e228",

    # Sister helpers cited from 800589d0 / 80059280 / 80060944 / 8005fb84:
    "800597c8",
    "8005ace8",
    "800608e0",
    "80059568",
    "80059634",
    "80059700",
    "80060a04",
    "8005fccc",

    # Misc 1-cite helpers from useful callers:
    "80057914",  # cited from 800468a4
    "80057c44",  # cited from 8001e1b4
    "8005800c",  # cited from 8001e1b4
    "80058068",  # cited from 8001e1b4
    "80055b4c",  # cited from battle state-machine path
    "80056b18",  # speculative - in same cluster
    "8005b0b8",  # cited from overlay_0896_801cb4a8
    "8005b618",  # cited from 80026f50 (renderer)
    "80046494",  # cited from overlay_801d0520
    "80046978",  # cited from 80016444 (early init)

    # Animation-tick chain (cited from 8001e1b4 actor scheduler):
    "80058f1c",  # cited from overlay_0978_801c39b8
    "80058fa0",  # cited from overlay_0978_801c39b8
    "8004695c",  # cited from overlay_0978_801c39b8
]

OVERLAY_0897_TARGETS = [
    # The 0897 town overlay cites these from the dialog opener path:
    "800349ec",
    "80035ea8",
    "800431d0",
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
elif "overlay_0897" in prog_name:
    for t in OVERLAY_0897_TARGETS:
        dump(t, prefix="overlay_0897")
else:
    print("(no targets for this program; skipping)")

print("done")
