# @category Legaia
# @runtime Jython
#
# Round 25: final 56-helper batch. Same enclosing-function logic as
# round 24. Run from 0896 program (contains full 0897 range too).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801e7448", "80019788", "801ce850", "801ce8a0", "801ce8cc",
    "801ce8ec", "801cea3c", "801cea6c", "801cec94", "801cee80",
    "801cef54", "801cf00c", "801cf070", "801cf1b0", "801cf4ac",
    "801cf5d0", "801cfa48", "801cfbe4", "801d0338", "801d08e4",
    "801d30b4", "801d31d8", "801d34a4", "801d5ae0", "801d629c",
    "801d6574", "801d95a8", "801dabb4", "801dac78", "801db2fc",
    "801db49c", "801db4e8", "801db6cc", "801db844", "801dba78",
    "801dbbcc", "801dbbdc", "801dbbfc", "801dcf24", "801dd094",
    "801dd0bc", "801e0b40", "801e92dc", "801e93c8", "801ea5c4",
    "801ec784", "801ecd0c", "801ef228", "801f03c0", "801f1cc8",
    "801f1fc8", "801f20dc", "801f4318", "801fd150", "802028c4",
    "80205504",
]

OUT_DIR = "/scripts/funcs"

prog = currentProgram
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

PREFIX = "overlay_0896_" if "0896" in prog_name else "overlay_0897_"
print("active program: {} (prefix={})".format(prog_name, PREFIX))


def find_enclosing(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        return None
    return fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)


decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def write_full_dump(func, out_path):
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    entry_str = "%08x" % func.getEntryPoint().getOffset()
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(
            func.getName(), entry_str, func.getEntryPoint()))
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


def write_pointer_stub(addr_str, func, stub_path):
    entry_str = "%08x" % func.getEntryPoint().getOffset()
    with open(stub_path, "w") as fh:
        fh.write("== {} (cite of {}) ==\n".format(
            addr_str, func.getName()))
        fh.write("This address is inside {} at entry {}.\n".format(
            func.getName(), entry_str))
        fh.write("See {}{}.txt for the full disassembly.\n".format(
            PREFIX, entry_str))


count_full = 0
count_stub = 0
count_skip = 0
for addr_str in TARGETS:
    func = find_enclosing(addr_str)
    if func is None:
        count_skip += 1
        continue
    entry_str = "%08x" % func.getEntryPoint().getOffset()
    full_path = os.path.join(OUT_DIR, PREFIX + entry_str + ".txt")
    stub_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    if not os.path.exists(full_path):
        write_full_dump(func, full_path)
        print("[full] {} ({})".format(full_path, func.getName()))
        count_full += 1
    if entry_str != addr_str and not os.path.exists(stub_path):
        write_pointer_stub(addr_str, func, stub_path)
        print("[stub] {} -> {}".format(stub_path, entry_str))
        count_stub += 1

print("done: {} full, {} stubs, {} skipped".format(count_full, count_stub, count_skip))
