# @category Legaia
# @runtime Jython
#
# Round 26: try CreateFunctionCmd at addresses Ghidra hasn't analyzed.
# Skip if the resulting function is <8 instructions (phantom stub).
# Short decompile timeout (15s) so failures don't hang the run.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801df098", "80019788", "801ce850", "801ce8a0", "801ce8cc",
    "801ce8ec", "801cea3c", "801cea6c", "801cec94", "801cee80",
    "801cef54", "801cf00c", "801cf070", "801cf4ac", "801cfa48",
    "801cfbe4", "801d6574", "801d8a2c", "801d9860", "801e205c",
    "801ea5c4", "801ea9b0", "801ec784", "801ecd0c", "801ef228",
    "801f04b0", "801f1cc8", "801f1fc8", "801f20dc", "801f4318",
    "801fd150", "802028c4", "80205504",
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


def find_or_create(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        return None
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is not None:
        return func
    try:
        from ghidra.app.cmd.function import CreateFunctionCmd
        cmd = CreateFunctionCmd(addr)
        if cmd.applyTo(prog, monitor):
            func = fm.getFunctionAt(addr)
            if func is None:
                return None
            # Reject phantom stubs - real functions have multiple instructions
            instrs = list(listing.getInstructions(func.getBody(), True))
            if len(instrs) < 8:
                # Remove the phantom; we don't want noise in the project
                fm.removeFunction(func.getEntryPoint())
                return None
            return func
    except Exception as e:
        print("[create-exc] {}: {}".format(addr_str, e))
    return None


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
            res = decomp.decompileFunction(func, 15, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))


def write_pointer_stub(addr_str, func, stub_path):
    entry_str = "%08x" % func.getEntryPoint().getOffset()
    with open(stub_path, "w") as fh:
        fh.write("== {} (cite of {}) ==\n".format(addr_str, func.getName()))
        fh.write("This address is inside {} at entry {}.\n".format(
            func.getName(), entry_str))
        fh.write("See {}{}.txt for the full disassembly.\n".format(
            PREFIX, entry_str))


count_full = 0
count_stub = 0
count_skip = 0
for addr_str in TARGETS:
    func = find_or_create(addr_str)
    if func is None:
        count_skip += 1
        continue
    entry_str = "%08x" % func.getEntryPoint().getOffset()
    full_path = os.path.join(OUT_DIR, PREFIX + entry_str + ".txt")
    stub_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    if not os.path.exists(full_path):
        write_full_dump(func, full_path)
        print("[full] {} ({}, {} instrs)".format(
            full_path, func.getName(),
            len(list(listing.getInstructions(func.getBody(), True)))))
        count_full += 1
    if entry_str != addr_str and not os.path.exists(stub_path):
        write_pointer_stub(addr_str, func, stub_path)
        print("[stub] {} -> {}".format(stub_path, entry_str))
        count_stub += 1

print("done: {} full, {} stubs, {} skipped".format(count_full, count_stub, count_skip))
