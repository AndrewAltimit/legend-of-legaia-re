# @category Legaia
# @runtime Jython
#
# Force-create functions at known overlay addresses then dump them.
# 0x801DE914 / 0x801E0088 / 0x801F5D90 are reachable only via JALR
# (computed jumps from a dispatch table) so Ghidra's auto-analyzer doesn't
# turn them into functions on its own.

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801de914",
    "801dfdf8",
    "801e0088",
    "801f5d90",
    "801f17f8",
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


def ensure_function(addr_str):
    addr = af.getAddress(addr_str)
    func = fm.getFunctionAt(addr)
    if func is not None:
        return func
    # Disassemble first if no instruction exists
    if listing.getInstructionAt(addr) is None:
        DisassembleCommand(addr, None, True).applyTo(prog, monitor)
    # Create the function (auto-discovers body via control flow)
    CreateFunctionCmd(addr).applyTo(prog, monitor)
    func = fm.getFunctionAt(addr)
    return func


def dump(addr_str):
    func = ensure_function(addr_str)
    if func is None:
        print("[fail] could not create function at {}".format(addr_str))
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
            res = decomp.decompileFunction(func, 180, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    print("wrote {} ({} bytes)".format(out_path, body.getNumAddresses()))


for t in TARGETS:
    dump(t)

print("done")
