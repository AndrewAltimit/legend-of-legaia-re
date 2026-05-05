# @category Legaia
# @runtime Jython
#
# Round 14: target the largest concentrated remaining cluster of missing
# helpers (SCUS 0x80061-0x8006C, which is the libgs/libsnd/libcd PsyQ
# region) plus the few SCUS singletons cited from already-decompiled
# functions. Each address listed is currently in the missing-helpers
# set returned by `scripts/function-coverage.py`.
#
# Most of these are statically-linked Sony PsyQ runtime routines (libgs
# primitives, libcd file-system, libsnd SsAPI, libapi BIOS shims) - they
# matter because every higher-level Legaia helper that calls them gets
# read more clearly once their semantics are pinned down by name.
#
# Routing: every address below is in 0x80010000-0x8006FFFF, so they
# belong in the SCUS_942.54 program. Run from the Ghidra container with
# the SCUS program open.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    # 1f/37/3f singletons (mostly file-IO / mode helpers)
    "8001fa68",  # cited from 8003f3fc
    "800379a8",  # cited from overlay_0896_801c8400
    "8003f3fc",  # paired with 8003f348 (we already have 8003f348)
    "8003f838",  # cited from 8003f3fc
    "8003f86c",  # cited from 8003f3fc

    # 48 (battle helpers)
    "80048310",  # cited from 8005112c
    "800485bc",  # cited from 80048310

    # 56-5f libcd cluster
    "80056678",  # cited from 8005bb48
    "80056688",  # cited from 8005bb48
    "80056748",  # cited from 8005de80, 8005dea0
    "80056768",  # cited from 800567b8, 80056b18
    "80057014",
    "80057024",
    "80057fec",  # cited from 8005724c
    "8005a78c",
    "8005abd0",
    "8005acac",
    "8005acd8",
    "8005af0c",
    "8005b7c0",
    "8005b7cc",
    "8005bb48",
    "8005bbe8",
    "8005c2c4",
    "8005ccb4",
    "8005d9a0",
    "8005e4d4",
    "8005e540",
    "8005fd68",
    "8005fd78",
    "8005fd88",
    "8005ff04",

    # 61-67 libgs / GPU primitive cluster
    "80061d18",
    "80061e94",
    "80061edc",
    "8006206c",
    "80062340",
    "80062410",
    "8006275c",
    "8006282c",
    "80062880",
    "800628f0",
    "80062aa0",
    "800641ec",
    "80065440",
    "80065978",
    "80065b88",
    "80066e50",
    "80067550",
    "80067e9c",

    # 68-69 libsnd SsAPI continuation + rng
    "800683d8",
    "800684cc",
    "80068b98",
    "80068c5c",
    "80068c70",
    "80068c80",
    "80068d34",
    "80069170",
    "80069230",
    "80069390",

    # 6a libcd phase ops
    "8006a7a4",
    "8006aa90",
    "8006acbc",

    # 6c misc tail (printf / libapi shims at 80018f94, plus a libgs follower)
    "8006c048",
    "8006ca7c",
    "8006cb3c",
    "8006cdb0",
    "8006ce30",
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


def dump(addr_str):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip] {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, "{}.txt".format(addr_str))
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
        dump(t)
else:
    print("(no targets for this program; skipping)")

print("done")
