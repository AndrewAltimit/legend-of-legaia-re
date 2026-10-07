# @category Legaia
# @runtime Jython
#
# Dumps disassembly + decompiled C for the six remaining "missing helper"
# entry points the function-coverage tracker reports. Run after the main
# `dump_funcs.py` to close out coverage to 100%.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

PROGRAM = "SCUS_942.54"

TARGETS_SCUS = [
    "8004ad80",  # cited from 80047430
    "8004c7b4",  # cited from 80047430
    "800508dc",  # cited from 80047430
    "80050e00",  # cited from 80047430
    # Round 2 - surfaced by adding the round-1 dumps.
    "8002b28c",  # cited from 8004ad80
    "8003cb54",  # cited from 8004ad80
    "8004c140",  # cited from 8004ad80
    "8004c650",  # cited from 8004ad80
    "8004e13c",  # cited from 8004ad80
]

TARGETS_OVERLAY_0897 = [
    # Cited only inside the 0897 town overlay.
    "801df510",
    "801e9d8c",
    # Round 2 - surfaced by the round-1 0897 dumps.
    "801d3730",
]


def dump_targets(program_name, targets):
    program = state.getCurrentProgram()
    if program.getName() != program_name:
        print("[skip] program is %s, expected %s" % (program.getName(), program_name))
        return
    decomp = DecompInterface()
    options = DecompileOptions()
    decomp.setOptions(options)
    decomp.openProgram(program)
    monitor = ConsoleTaskMonitor()
    out_dir = "/scripts/funcs"
    if not os.path.isdir(out_dir):
        os.makedirs(out_dir)
    for hex_addr in targets:
        addr = program.getAddressFactory().getAddress("0x" + hex_addr)
        func = program.getFunctionManager().getFunctionAt(addr)
        if func is None:
            print("[skip] no function at 0x%s" % hex_addr)
            continue
        # Disassembly
        listing = program.getListing()
        instrs = listing.getInstructions(func.getBody(), True)
        lines = []
        lines.append("== %s 0x%s (entry=0x%s) ==" % (func.getName(), hex_addr, hex_addr))
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
        # Decompile
        result = decomp.decompileFunction(func, 60, monitor)
        if result is not None and result.getDecompiledFunction() is not None:
            lines.append(result.getDecompiledFunction().getC())
        else:
            lines.append("(decompile failed)")
        out_path = os.path.join(out_dir, hex_addr + ".txt")
        with open(out_path, "w") as f:
            f.write("\n".join(lines))
        print("wrote %s" % out_path)


# Caller picks which set to dump via OVERLAY_PROGRAM env var.
target_program = os.environ.get("OVERLAY_PROGRAM", PROGRAM)
if target_program == PROGRAM:
    dump_targets(PROGRAM, TARGETS_SCUS)
else:
    dump_targets(target_program, TARGETS_OVERLAY_0897)
