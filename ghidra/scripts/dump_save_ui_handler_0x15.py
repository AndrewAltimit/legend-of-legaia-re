# @category Legaia
# @runtime Jython
#
# Dumps the save-screen sub-state 0x15 handler at 0x801DA2A0.
# This is the only entry in the PTR_FUN_801e4f40 dispatch table that
# did not have a prior dump.
#
# Run against overlay_shop_save.bin (the menu overlay):
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_shop_save.bin -noanalysis \
#       -postScript /scripts/dump_save_ui_handler_0x15.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.util.task import ConsoleTaskMonitor

PROGRAM = "overlay_shop_save.bin"
TARGET = "801da2a0"

program = state.getCurrentProgram()
if program.getName() != PROGRAM:
    print("[skip] program is %s, expected %s" % (program.getName(), PROGRAM))
else:
    addr = program.getAddressFactory().getAddress("0x" + TARGET)
    func = program.getFunctionManager().getFunctionAt(addr)
    if func is None:
        cmd = CreateFunctionCmd(addr)
        cmd.applyTo(program, ConsoleTaskMonitor())
        func = program.getFunctionManager().getFunctionAt(addr)
    if func is None:
        print("[fail] could not create function at 0x%s" % TARGET)
    else:
        decomp = DecompInterface()
        options = DecompileOptions()
        decomp.setOptions(options)
        decomp.openProgram(program)
        monitor = ConsoleTaskMonitor()
        listing = program.getListing()
        instrs = listing.getInstructions(func.getBody(), True)
        lines = []
        lines.append("== %s 0x%s (entry=0x%s) [%s] ==" % (
            func.getName(), TARGET, TARGET, program.getName()))
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
        out_path = os.path.join("/scripts/funcs",
                                "overlay_shop_save_" + TARGET + ".txt")
        with open(out_path, "w") as f:
            f.write("\n".join(lines))
        print("wrote %s" % os.path.basename(out_path))
