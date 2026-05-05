# @category Legaia
# @runtime Jython
#
# Round 17: residual SCUS sequencer helpers surfaced by round-16 dumps.
# 80069da8 cites FUN_800697e0 directly; 8006a158 / 8006a420 cite the
# 8006a020 / 8006a04c neighbours. Each is a 1-ref tail on the libsnd
# SsAPI cluster.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    "800697e0",
    "8006a020",
    "8006a04c",
    "8006a078",
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
    func = fm.getFunctionAt(addr)
    if func is None:
        try:
            from ghidra.app.cmd.disassemble import DisassembleCommand
            from ghidra.program.model.address import AddressSet

            cmd = DisassembleCommand(AddressSet(addr), None, True)
            cmd.applyTo(prog, monitor)
        except Exception:
            pass
        try:
            from ghidra.app.cmd.function import CreateFunctionCmd

            cmd = CreateFunctionCmd(addr, False)
            cmd.applyTo(prog, monitor)
            func = fm.getFunctionAt(addr)
        except Exception:
            pass
    return func


def dump_function(addr_str):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip] no function at " + addr_str)
        return False

    out_path = os.path.join(OUT_DIR, addr_str + ".txt")
    if os.path.exists(out_path):
        print("[skip-exists] " + addr_str)
        return True

    res = decomp.decompileFunction(func, 60, monitor)
    if not res.decompileCompleted():
        print("[fail-decompile] " + addr_str + " : " + res.getErrorMessage())
        return False

    body = func.getBody()
    instrs = listing.getInstructions(body, True)

    parts = []
    parts.append("=" * 72)
    parts.append("Function: " + func.getName() + " @ " + addr_str)
    parts.append("Program: " + prog_name)
    parts.append("Body bytes: " + str(body.getNumAddresses()))
    parts.append("=" * 72)
    parts.append("")
    parts.append("--- Decompile ---")
    parts.append(res.getDecompiledFunction().getC())
    parts.append("")
    parts.append("--- Disassembly ---")
    for ins in instrs:
        parts.append("%s  %s" % (str(ins.getAddress()), ins.toString()))

    with open(out_path, "w") as f:
        f.write("\n".join(parts))
    print("[ok] " + addr_str + " -> " + out_path)
    return True


for addr in SCUS_TARGETS:
    dump_function(addr)
