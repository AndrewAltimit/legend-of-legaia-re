# @category Legaia
# @runtime Jython
#
# Dumps the 16 remaining functions from overlay_battle_action.bin that are
# not yet represented under ghidra/scripts/funcs/overlay_battle_action_<addr>.txt.
#
# Missing as of 2026-05-08 (comm of inventory vs dumped):
#   801d5718  801d5778  801d57e8  801d9ae8  801da6b4  801db7b0
#   801db81c  801db8b4  801db8f4  801db9c4  801dba04  801dbb8c
#   801dbd04  801dbddc  801dbec4  801e7250
#
# These are all small helpers (52..204 bytes) called by the action-dispatch
# subroutines already dumped.  They finish the coverage of the 78-function
# overlay.
#
# Run against the battle_action named program:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_battle_action.bin -noanalysis \
#       -postScript /scripts/dump_remaining_battle_action.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d5718",   # 96 bytes,  18 callers -- small display helper
    "801d5778",   # 112 bytes,  2 callers
    "801d57e8",   # 108 bytes,  2 callers
    "801d9ae8",   # 212 bytes,  1 caller
    "801da6b4",   # 204 bytes,  9 callers
    "801db7b0",   # 108 bytes,  1 caller
    "801db81c",   # 152 bytes, 10 callers
    "801db8b4",   # 64 bytes,   8 callers
    "801db8f4",   # 208 bytes,  6 callers
    "801db9c4",   # 64 bytes,   1 caller
    "801dba04",   # 140 bytes,  3 callers
    "801dbb8c",   # 164 bytes,  3 callers
    "801dbd04",   # 216 bytes,  1 caller
    "801dbddc",   # 232 bytes,  4 callers
    "801dbec4",   # 216 bytes,  1 caller
    "801e7250",   # 208 bytes,  1 caller
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
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    block = mem.getBlock(addr)
    return block is not None


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
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
    finally:
        fh.close()
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done [{}]".format(prog_name))
