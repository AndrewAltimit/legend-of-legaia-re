# @category Legaia
# @runtime Jython
#
# Round 24: final cleanup of all 94 remaining missing helpers.
#
# Splits across two overlay programs:
#   - 0896 prefix range (0x801C5818-0x801CE818) - dump from 0896 program
#   - 0x801CE818+                                 - dump from 0897 program
#
# For each cited address:
#   - If a function CONTAINS it (e.g. mid-function citation), write both
#     <cited>.txt (a one-line pointer to the entry) AND <entry>.txt (the
#     full function dump). The cited-addr file satisfies the coverage
#     tracker, which key-matches by filename addr.
#   - If no enclosing function, skip silently. We do NOT create 1-byte
#     phantom functions via CreateFunctionCmd - those just hang the
#     decompiler at 5min/each and produce no useful output.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801c5cf8", "801c5e28", "801c63f0", "801c6884", "801c735c",
    "801c7574", "801c8178", "801c82e0", "801c8ae8", "801c906c",
    "801c924c", "801c9394", "801c9608", "801c97dc", "801c9a00",
    "801c9e3c", "801ca278", "801cac44", "801cae64", "801cc810",
    "801cce38", "801cd194", "801cd1d8", "801cd21c", "801cd260",
    "801cd2a4", "801cd2e8", "801cd3b4", "801cd40c", "801cd4ec",
    "801cd510", "801cd628", "801cd728", "801cd844", "801cd9c0",
    "801cdafc", "801cdb1c", "801cdb48", "801ce850", "801ce8a0",
    "801ce8cc", "801ce8ec", "801cea3c", "801cea6c", "801cec94",
    "801cee80", "801cef54", "801cf00c", "801cf070", "801cf1b0",
    "801cf4ac", "801cf5d0", "801cf9f4", "801cfa48", "801cfbe4",
    "801cfe4c", "801d6574", "801d8894", "801e6a7c", "801e76d4",
    "801e7824", "801e791c", "801e8b34", "801e92dc", "801e93c8",
    "801e9dc8", "801e9f64", "801ea074", "801ea348", "801ea5c4",
    "801ea7ac", "801ec0dc", "801ec228", "801ec784", "801ecd0c",
    "801eed1c", "801ef228", "801ef648", "801ef6e0", "801ef7b4",
    "801ef9b4", "801ef9e4", "801efbfc", "801efe44", "801f02d0",
    "801f1cc8", "801f1fc8", "801f20dc", "801f4318", "801f452c",
    "801f69d8", "801fd150", "802028c4", "80205504",
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

PREFIX = "overlay_0896_" if "0896" in prog_name else "overlay_0897_"
print("active program: {} (prefix={})".format(prog_name, PREFIX))


def find_enclosing(addr_str):
    """Return the Function whose body contains addr_str, or None.
    Does NOT create new functions - cited mid-function addresses get
    mapped to their enclosing function instead."""
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
    """Write a minimal file at <addr_str>.txt that references the enclosing
    function. This satisfies the coverage tracker without re-decompiling."""
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

print("done: {} full dumps, {} stubs, {} skipped (no enclosing function)".format(
    count_full, count_stub, count_skip))
