# @category Legaia
# @runtime Jython
#
# Dump the eight overlay-resident high-mode renderers swapped in by the
# world-map top-view via FUN_80043390's overlay path. Addresses are the
# `--overlay-targets-only` output of `mednafen-state prim-dispatch-table`
# against a save state that has the world-map overlay paged in.
#
# Each leaf is the SCUS-side high-mode renderer body plus a distance-cue
# fog post-process (GTE.dpcs / dpct) that tints per-vertex colors via a
# per-Z LUT. The dispatch flag `_DAT_1F800394 & 1` selects the overlay
# table.
#
# Run inside Ghidra against the overlay program imported via
# scripts/ghidra-analysis/import-overlay-named.sh. Output lands at
# ghidra/scripts/funcs/<prog>_<addr>.txt.
#
# Pair this with `dump_funcs.py` against SCUS_942.54 to compare each
# overlay leaf to its SCUS sibling at slot N of alpha row 0
# (SCUS_TABLE_BASE = 0x8007657C). The two are deliberately structurally
# parallel: SCUS_slot N has the same vertex-fetch + GTE projection +
# OT-packet write as overlay_leaf N, with the fog block omitted.

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# The eight overlay-resident high-mode targets, ordered by slot index
# (12..19). Matches `mednafen-state prim-dispatch-table --overlay-targets-only`.
TARGETS = [
    "801F7644",  # slot 12
    "801F7838",  # slot 13
    "801F7AA4",  # slot 16  (per-byte-order in the table; not 14)
    "801F7CCC",  # slot 17
    "801F7F78",  # slot 14
    "801F8198",  # slot 15
    "801F8454",  # slot 18
    "801F8690",  # slot 19
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
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def out_path_for(addr_str):
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    block = mem.getBlock(addr)
    return block is not None


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        # The leaves are branched-to fragments (`j 0x80043580` tail
        # call, no jr ra), so Ghidra may not have auto-defined them as
        # functions. Force a disassembly + CreateFunctionCmd which
        # auto-detects the body via flow analysis.
        DisassembleCommand(addr, None, True).applyTo(prog, monitor)
        CreateFunctionCmd(addr).applyTo(prog, monitor)
        func = fm.getFunctionAt(addr) or fm.getFunctionContaining(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
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
    finally:
        fh.close()
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done [{}]".format(prog_name))
