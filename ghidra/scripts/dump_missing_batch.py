# @category Legaia
# @runtime Jython
# Dump a list of cited helpers from the active program. Reads addresses
# from /scripts/missing_addrs.txt (one per line, lowercase hex). For each
# address: if a function exists at that address, dump it; otherwise find
# the enclosing function and dump that, plus a small stub note for the
# cited address.

import os
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

OUT_DIR = "/scripts/funcs"
ADDRS_FILE = "/scripts/missing_addrs.txt"

prog = currentProgram
prog_name = prog.getName()

# The originals use prefixes like "overlay_battle_action_" or just
# "<addr>.txt" for the SCUS program.
if prog_name == "SCUS_942.54":
    PREFIX = ""
elif prog_name.startswith("overlay_"):
    PREFIX = prog_name.replace(".bin", "") + "_"
else:
    PREFIX = "overlay_" + prog_name.replace(".bin", "") + "_"

fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def addr(s):
    return af.getAddress(s)


def dump_func(func, dest_name):
    out_path = os.path.join(OUT_DIR, dest_name)
    if os.path.exists(out_path):
        return False
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    if len(instrs) < 4:
        return False
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(
            func.getName(), "%08x" % func.getEntryPoint().getOffset(),
            func.getEntryPoint()))
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
    print("dumped " + dest_name)
    return True


def write_stub(addr_str, parent_addr_str):
    out_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    if os.path.exists(out_path):
        return False
    with open(out_path, "w") as fh:
        fh.write("== citation pointer 0x{} ==\n".format(addr_str))
        fh.write("Mid-function citation. Enclosing function dumped as ")
        fh.write("{}{}.txt\n".format(PREFIX, parent_addr_str))
    print("stubbed " + addr_str)
    return True


count_dumped = 0
count_stubbed = 0
count_skip = 0

with open(ADDRS_FILE) as fh:
    target_addrs = [line.strip() for line in fh if line.strip()]

for hex_str in target_addrs:
    a = addr(hex_str)
    if a is None:
        continue
    # Is the address even mapped in this program?
    if listing.getInstructionAt(a) is None and listing.getDataAt(a) is None:
        # Try to find enclosing fn anyway via FunctionManager
        f = fm.getFunctionContaining(a)
        if f is None:
            count_skip += 1
            continue
    f = fm.getFunctionAt(a)
    if f is not None:
        if dump_func(f, PREFIX + hex_str + ".txt"):
            count_dumped += 1
        continue
    f = fm.getFunctionContaining(a)
    if f is not None:
        parent_str = "%08x" % f.getEntryPoint().getOffset()
        if dump_func(f, PREFIX + parent_str + ".txt"):
            count_dumped += 1
        if hex_str != parent_str:
            if write_stub(hex_str, parent_str):
                count_stubbed += 1
    else:
        count_skip += 1

print("done: {} dumped, {} stubbed, {} not in this program".format(
    count_dumped, count_stubbed, count_skip))
