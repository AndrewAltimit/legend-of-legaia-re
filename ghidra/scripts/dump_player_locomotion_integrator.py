# @category Legaia
# @runtime Jython
#
# Dump the functions that WRITE the player actor world position
# (player[+0x14] = X, player[+0x18] = Z), pinned by the runtime
# write-watchpoint probe autorun_player_pos_watch.lua:
#
#   overlay 0897:  0x801D0684 / 06E4 / 0744 / 07B4  (read held pad
#                  _DAT_8007B850, add/sub a step to player X/Z) and
#                  0x801D1AD4..1B00 (secondary writer)
#   SCUS:          0x80020E3C / 0x800212C4 (per-frame transform commit)
#
# `in_program` skips addresses not in the current program, so run once
# per program (SCUS + overlay_0897):
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/dump_player_locomotion_integrator.py
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0897.bin.0 -noanalysis \
#       -postScript /scripts/dump_player_locomotion_integrator.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = ["801d0684", "801d1ad4", "80020e3c", "800212c4",
           "801cfe4c", "801cf9f4", "80046494", "800467e8"]

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
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None or not in_program(addr):
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    fh = open(out_path_for(addr_str), "w")
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
            res = decomp.decompileFunction(func, 90, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    print("wrote {}".format(out_path_for(addr_str)))


for t in TARGETS:
    dump(t)
print("done [{}]".format(prog_name))
