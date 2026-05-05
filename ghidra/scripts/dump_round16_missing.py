# @category Legaia
# @runtime Jython
#
# Round 16: the last 7 missing SCUS helpers reported by
# `scripts/function-coverage.py`. All cluster in 0x80056-0x8006c, which
# is the libsnd SsAPI / libcd / libapi region exposed in earlier rounds.
# Each is cited from one or two helpers we already dumped, so the residual
# missing-helper count after round 16 should fall to the overlay-only
# tail (the 0896 options-menu cluster).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    "80056658",
    "80069b18",
    "80069da8",
    "8006a158",
    "8006a420",
    "8006b844",
    "8006bc9c",
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
        # Try to disassemble; some addresses may be in a code region that
        # Ghidra hasn't promoted to a function yet.
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
