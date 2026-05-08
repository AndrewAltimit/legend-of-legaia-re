# @category Legaia
# @runtime Jython
#
# Dumps the 7 SCUS-range functions that are the last missing entries in the
# function-coverage tracker (as of 2026-05-08).  Run against SCUS_942.54.
#
# Missing SCUS addresses:
#   8003d038 - 3 refs from cutscene overlays (world-map sprite batcher)
#   8001fa34 - 2 refs from cutscene overlays (sprite-list search/init)
#   8005ba68 - 2 refs from cutscene overlays (libgte cluster neighbor of BA1C)
#   8006d7a4 - 1 ref from 8006d2ac (audio helper)
#   8006ef68 - 1 ref from 8006ef18 (SPU init trio sub-1)
#   8006f088 - 1 ref from 8006ef18 (SPU init trio sub-2)
#   8006f118 - 1 ref from 8006ef18 (SPU init trio sub-3)
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/dump_final_8_helpers.py
#
# Output: ghidra/scripts/funcs/<addr>.txt per entry.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.util.task import ConsoleTaskMonitor

PROGRAM = "SCUS_942.54"

TARGETS = [
    # Cited by cutscene overlays (world-map sprite batcher / camera)
    "8003d038",  # 3 refs from 801cfc40 -- takes u16 from actor+0x50
    "8001fa34",  # 2 refs from 801d629c -- sprite-list search (base, base+4, count)
    "8005ba68",  # 2 refs from 801d095c -- libgte cluster (neighbor of 8005BA1C)
    # Audio / SPU cluster
    "8006d7a4",  # 1 ref from 8006d2ac -- audio init helper
    "8006ef68",  # 1 ref from 8006ef18 -- SPU init trio sub-1
    "8006f088",  # 1 ref from 8006ef18 -- SPU init trio sub-2
    "8006f118",  # 1 ref from 8006ef18 -- SPU init trio sub-3
]


def dump_one(program, hex_addr, decomp, monitor, out_dir):
    addr = program.getAddressFactory().getAddress("0x" + hex_addr)
    func = program.getFunctionManager().getFunctionAt(addr)
    if func is None:
        cmd = CreateFunctionCmd(addr)
        cmd.applyTo(program, monitor)
        func = program.getFunctionManager().getFunctionAt(addr)
    if func is None:
        print("[skip] could not create function at 0x%s in %s" % (hex_addr, program.getName()))
        return
    listing = program.getListing()
    instrs = listing.getInstructions(func.getBody(), True)
    lines = []
    lines.append("== %s 0x%s (entry=0x%s) [%s] ==" % (
        func.getName(), hex_addr, hex_addr, program.getName()))
    size = func.getBody().getNumAddresses()
    n_instr = 0
    instr_lines = []
    for instr in instrs:
        n_instr += 1
        instr_lines.append("%s  %s" % (instr.getAddress(), instr.toString()))
    lines.append("size=%d bytes, %d instructions" % (size, n_instr))
    lines.append("")
    lines.append("--- DISASSEMBLY ---")
    lines.extend(instr_lines)
    lines.append("")
    lines.append("--- DECOMPILED ---")
    result = decomp.decompileFunction(func, 60, monitor)
    if result is not None and result.getDecompiledFunction() is not None:
        lines.append(result.getDecompiledFunction().getC())
    else:
        lines.append("(decompile failed)")
    out_path = os.path.join(out_dir, hex_addr + ".txt")
    with open(out_path, "w") as f:
        f.write("\n".join(lines))
    print("wrote %s" % out_path)


program = state.getCurrentProgram()
if program.getName() != PROGRAM:
    print("[skip] program is %s, expected %s" % (program.getName(), PROGRAM))
else:
    decomp = DecompInterface()
    options = DecompileOptions()
    decomp.setOptions(options)
    decomp.openProgram(program)
    monitor = ConsoleTaskMonitor()
    out_dir = "/scripts/funcs"
    if not os.path.isdir(out_dir):
        os.makedirs(out_dir)
    for hex_addr in TARGETS:
        dump_one(program, hex_addr, decomp, monitor, out_dir)
