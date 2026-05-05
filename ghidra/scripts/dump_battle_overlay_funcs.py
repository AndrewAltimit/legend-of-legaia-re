# @category Legaia
# @runtime Jython
#
# Dump disassembly + decompiled C for the effect-bundle consumer cluster
# living in the imported battle overlay. Output names are prefixed
# `overlay_battle_` to keep them distinct from title/town overlay dumps.
#
# Run with:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay.bin -noanalysis \
#       -postScript /scripts/dump_battle_overlay_funcs.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # --- Confirmed effect-bundle consumer cluster ---
    "801de914",  # init / pack-fixup; called by SCUS FUN_800520F0 case 0xe
    "801dfdf8",  # effect dispatcher; jumps into 0x801F5D90
    "801e0088",  # per-frame walker (0x970 bytes) - the prize target
    # --- Cross-jump target in the previously-unseen 0x801F0000+ region ---
    "801f5d90",
    # --- Other 0x801F0000+ functions reached from 801E0088 (best guesses) ---
    "801f17f8",
    # --- Surrounding context: functions that may be siblings in the same
    # subsystem (callees / callers of the cluster). These are speculative;
    # cull from output if they turn out unrelated.
    "801de7f0",  # likely sibling helper just before init
    "801dff48",  # between init and dispatcher
    "801e09f8",  # next func after walker
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
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        func = fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, "overlay_battle_" + addr_str + ".txt")
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
            res = decomp.decompileFunction(func, 120, monitor)
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
