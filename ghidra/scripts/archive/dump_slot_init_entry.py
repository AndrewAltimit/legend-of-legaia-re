# @category Legaia
# @runtime Jython
#
# One-off: dump the slot-machine overlay init entry FUN_801cec94 (the
# mode-24 warp target; LCG seed + balance-from-coin-bank seed). Creates the
# function first if the headless import never defined one there.
#
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_slot_machine.bin -noanalysis \
#       -postScript /scripts/dump_slot_init_entry.py

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGET = "801cec94"
OUT_DIR = "/scripts/funcs"

prog = currentProgram
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

addr = af.getAddress(TARGET)
func = fm.getFunctionAt(addr)
if func is None:
    DisassembleCommand(addr, None, True).applyTo(prog, monitor)
    CreateFunctionCmd(addr).applyTo(prog, monitor)
    func = fm.getFunctionAt(addr)
if func is None:
    print("[fail] could not create a function at " + TARGET)
else:
    decomp = DecompInterface()
    decomp.setOptions(DecompileOptions())
    decomp.openProgram(prog)
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    label = prog_name.replace(".bin", "").replace(".", "_")
    out_path = os.path.join(OUT_DIR, label + "_" + TARGET + ".txt")
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), TARGET, func.getEntryPoint(), prog_name))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        res = decomp.decompileFunction(func, 60, monitor)
        if res.decompileCompleted():
            fh.write(res.getDecompiledFunction().getC())
        else:
            fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
    finally:
        fh.close()
    print("wrote " + out_path)
