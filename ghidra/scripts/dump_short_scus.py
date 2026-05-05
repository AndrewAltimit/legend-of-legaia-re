# @category Legaia
# @runtime Jython
# Dump 3-instruction "functions" (thunks). Lower threshold than the
# main force_disasm_dump.py so we cover BIOS thunks and short wrappers.

import os
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.program.model.address import AddressSet

OUT_DIR = "/scripts/funcs"

# Short thunks identified by force_disasm_dump.py
ADDRS = [
    "800566a8", "800566b8", "800566c8", "800566d8", "800566e8",
    "800566f8", "80056708", "80056718", "80056698",
    "8006ee14", "8006ee24",
    "80019788", "80035c10",
]

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def addr(s):
    return af.getAddress(s)


def force_disasm(a):
    cmd = DisassembleCommand(a, None, True)
    return cmd.applyTo(prog, monitor)


def find_short_end(a, max_instrs=20):
    cur = a
    count = 0
    while count < max_instrs:
        ins = listing.getInstructionAt(cur)
        if ins is None:
            return None, count
        mnem = ins.getMnemonicString().lower()
        if mnem == "jr":
            cur = cur.add(4)
            return cur, count + 2
        cur = cur.add(4)
        count += 1
    return None, count


def dump_one(hs):
    a = addr(hs)
    if a is None:
        return False
    out_path = os.path.join(OUT_DIR, hs + ".txt")
    if os.path.exists(out_path):
        return False

    if listing.getInstructionAt(a) is None:
        if not force_disasm(a):
            print("skip {} (disasm failed)".format(hs))
            return False
    end_addr, n = find_short_end(a, 30)
    if end_addr is None or n < 2:
        print("skip {} (no end)".format(hs))
        return False

    f = fm.getFunctionAt(a)
    if f is None:
        body = AddressSet(a, end_addr.subtract(1))
        cmd = CreateFunctionCmd(None, a, body,
                                ghidra.program.model.symbol.SourceType.USER_DEFINED)
        if not cmd.applyTo(prog, monitor):
            print("skip {} (createfn failed)".format(hs))
            return False
        f = fm.getFunctionAt(a)
    if f is None:
        return False
    body = f.getBody()
    instrs = list(listing.getInstructions(body, True))
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(
            f.getName(), hs, f.getEntryPoint()))
        fh.write("size={} bytes, {} instructions (short thunk)\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(f, 30, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    print("dumped " + hs)
    return True


count = 0
for hs in ADDRS:
    if dump_one(hs):
        count += 1
print("done: {} short thunks dumped".format(count))
