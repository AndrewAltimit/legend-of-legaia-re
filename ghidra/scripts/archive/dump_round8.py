# @category Legaia
# @runtime Jython
#
# Round-8 batch: closes the citation graph from 21 -> 0 missing helpers.
# - OVERLAY_TARGETS: 16 functions in overlay_0897_xxx_dat.bin tail-called
#   from FUN_801cd8a4 + sibling 0897 helpers cited from the second-pass
#   tracker after the first batch surfaced new orphans.
# - SCUS_TARGETS: 2 BIOS B-vector thunks (8006ee6c/7c) - too short for
#   force_disasm_dump.py's 8-instruction floor.
# - OVERLAY_0896_TARGETS / OVERLAY_0978_TARGETS: short stubs reachable
#   only via mid-function `jal`s that Ghidra's auto-analysis didn't
#   promote to function entry points; dumped via /tmp/dump_short.py
#   one-shot. Kept here as a record of what was traced.
#
# Run mode: ROUND8_MODE env var ("scus", "ov0896", "ov0978", or default
# "overlay") picks which target list + filename prefix to use.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

OVERLAY_TARGETS = [
    # First batch (xxx_dat dispatcher tail) -- keeps for re-runs.
    "801d4908",
    "801d4ba0",
    "801dc320",
    "801dc4c0",
    "801dc6a0",
    "801dc6e0",
    "801dc8c0",
    "801dcaa0",
    "801dcc40",
    "801dd000",
    "801e00b8",
    # Second batch -- orphans cited from sibling 0897 dumps.
    "801f1fc8",
    "801f4318",
    "801fd150",
    "802028c4",
    "80205504",
]

# Used when running against SCUS_942.54
SCUS_TARGETS = [
    "8006ee6c",
    "8006ee7c",
]

# 0896-overlay-resident; cited by SCUS 80025980 (mode-24 OTHER init).
OVERLAY_0896_TARGETS = [
    "801cee80",
    "801cef54",
    "801cf00c",
]
# 0978-overlay-resident; cited by overlay_0978_801c39b8.
OVERLAY_0978_TARGETS = [
    "801cf1b0",
]

import sys
# Run mode picked from environment so the same script works across programs.
mode = os.environ.get("ROUND8_MODE", "overlay")
if mode == "scus":
    TARGETS = SCUS_TARGETS
    PREFIX_FOR_MODE = ""
elif mode == "ov0896":
    TARGETS = OVERLAY_0896_TARGETS
    PREFIX_FOR_MODE = "overlay_0896_"
elif mode == "ov0978":
    TARGETS = OVERLAY_0978_TARGETS
    PREFIX_FOR_MODE = "overlay_0978_"
else:
    TARGETS = OVERLAY_TARGETS
    PREFIX_FOR_MODE = "overlay_0897_xxx_dat_"

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
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] not an address: " + addr_str)
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        func = fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for " + addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, PREFIX_FOR_MODE + addr_str + ".txt")
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
    print("wrote " + out_path)


for t in TARGETS:
    dump(t)

print("done")
