# @category Legaia
# @runtime Jython
#
# Shared body of the per-overlay `dump_*_overlay.py` scripts: decompile a list
# of target addresses out of the current program and write one
# `<label>_<addr>.txt` per function (disassembly, then decompiled C) into
# /scripts/funcs. A per-overlay script keeps only its header comment and its
# TARGETS list and ends with
#
#     from lib_dump import dump_targets
#     dump_targets(currentProgram, TARGETS)
#
# Ghidra puts the script directory on the Jython path, so the import resolves
# from /scripts. A module cannot see the script's `currentProgram`, which is
# why it is passed in.
#
# Not a script to run on its own: it has no TARGETS.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

DEFAULT_OUT_DIR = "/scripts/funcs"


def out_path_for(out_dir, prog_name, addr_str, scus_bare):
    """`<program label>_<addr>.txt`, or bare `<addr>.txt` for SCUS when
    `scus_bare` (the executable's dumps are named by address alone)."""
    if scus_bare and prog_name.startswith("SCUS"):
        return os.path.join(out_dir, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(out_dir, label + "_" + addr_str + ".txt")


def dump_targets(prog, targets, out_dir=None, scus_bare=False):
    """Dump every address in `targets` that lies in `prog`'s memory.

    An address outside the program is skipped silently - the same TARGETS
    list is run against several overlay programs that share a VA band - and
    an address with no function is reported and skipped.
    """
    out_dir = out_dir or os.environ.get("LEGAIA_DUMP_OUT_DIR") or DEFAULT_OUT_DIR
    try:
        os.makedirs(out_dir)
    except OSError:
        pass
    prog_name = prog.getName()
    fm = prog.getFunctionManager()
    listing = prog.getListing()
    af = prog.getAddressFactory()
    mem = prog.getMemory()
    monitor = ConsoleTaskMonitor()
    decomp = DecompInterface()
    decomp.setOptions(DecompileOptions())
    decomp.openProgram(prog)

    for addr_str in targets:
        addr = af.getAddress(addr_str)
        if addr is None:
            print("[skip] {} not an address".format(addr_str))
            continue
        if mem.getBlock(addr) is None:
            continue
        func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
        if func is None:
            print("[skip] no function at {} in {}".format(addr_str, prog_name))
            continue

        body = func.getBody()
        instrs = list(listing.getInstructions(body, True))
        out_path = out_path_for(out_dir, prog_name, addr_str, scus_bare)
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
