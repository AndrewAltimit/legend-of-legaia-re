# @category Legaia
# @runtime Jython
#
# Round 13: follow-up batch.
# (a) The 800349ec / 80035ea8 / 800431d0 addresses cited from the 0897
#     town overlay are SCUS addresses (overlays jal back into SCUS for
#     dialog opener / mode helpers). Round 12 routed them to the wrong
#     program; do them in SCUS this time.
# (b) Freshly-revealed 2-cite helpers from round-12 dumps.
# (c) High-value 1-cite siblings from the same clusters.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    # (a) 0897-cited SCUS helpers (round 12 mis-routed):
    "800349ec",
    "80035ea8",
    "800431d0",

    # (b) Top-2-cite freshly-revealed:
    "80056748",  # cited from 8005de80, 8005dea0
    "80056768",  # cited from 800567b8, 80056b18
    "80057014",  # cited from 800567b8, 80056b18
    "80057024",  # cited from 800567b8, 80056b18
    "8005acac",  # cited from 80057c44, 80058068
    "8005b7c0",  # cited from 8005acf8, 8005ad54
    "8005b7cc",  # cited from 8005acf8, 8005ad54
    "8005e4d4",  # cited from 8005dea0, 8005e228
    "8005e540",  # cited from 8005dea0, 8005e228

    # (c) High-leverage 1-cites:
    "8003f3fc",  # cited from 8003f348 (paired)
    "80046898",  # cited from 8003fb10
    "80048310",  # cited from 8005112c
    "8004ccd4",  # cited from 80049348
    "800560b4",  # cited from 80034a6c
    "80057fec",  # cited from 8005724c
    "8005a78c",  # cited from 80057c44
    "8005acd8",  # cited from 80057c44
    "8005bb48",  # cited from 8005afb0
    "8005c2c4",  # cited from 8005e574
    "8005ccb4",  # cited from 8005beac
    "8005fd68",  # cited from 8005fccc
    "8005fd78",  # cited from 8005fccc
    "8005fd88",  # cited from 80057c44

    # GPU/scratch cluster 80061-80067 (cited from 80026410-800268xx renderer entry pts):
    "80061e94",
    "80061edc",
    "80062340",
    "8006282c",
    "80062880",
    "80062aa0",
    "800641ec",
    "80065440",
    "80065978",
    "80066e50",
    "80067550",
    "80067e9c",
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
else:
    print("(no targets for this program; skipping)")

print("done")
