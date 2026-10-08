# @category Legaia
# @runtime Jython
#
# Dump every world-map-top overlay function that calls FUN_80034B78 (the
# high-frequency drawable installer used by the top-view debug renderer).
# Also dump FUN_80034B78 itself from SCUS so we can see what it wraps.
#
# Together these tell us:
#  - Which world-map overlay functions install drawable nodes for the
#    continent / landmarks / camera setup.
#  - Whether FUN_80034B78 calls FUN_800326AC internally (the canonical
#    install fn) or maintains its own list.
#
# Output: /scripts/funcs/overlay_world_map_top_<addr>.txt for overlay
# functions, /scripts/funcs/<addr>.txt for SCUS functions.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)

PROGRAM_NAME = prog.getName()
OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass


def out_path_for(addr_str):
    # Match dump_pending_helpers.py prefix convention: overlay-resident
    # functions get tagged with the overlay program name.
    if "overlay" in PROGRAM_NAME.lower():
        # Strip ".bin" suffix.
        label = PROGRAM_NAME.replace(".bin", "")
        return os.path.join(OUT_DIR, "%s_%s.txt" % (label, addr_str))
    return os.path.join(OUT_DIR, "%s.txt" % addr_str)


def in_program(addr):
    return af.getAddress("%x" % addr.getOffset()) is not None


def dump_func_at(addr_int, label=""):
    addr_str = "%08x" % addr_int
    addr = af.getAddress(addr_str)
    if addr is None:
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        # Not in this program's address space - skip silently.
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out = out_path_for(addr_str)
    refs = list(ref_mgr.getReferencesTo(addr))
    with open(out, "w") as fh:
        fh.write("== %s %s (entry=%s) %s ==\n" % (
            func.getName(), addr_str, func.getEntryPoint(), label))
        fh.write("size=%d bytes, %d instructions, %d refs to entry\n\n" % (
            body.getNumAddresses(), len(instrs), len(refs)))
        fh.write("--- CALLERS ---\n")
        for r in refs:
            from_a = r.getFromAddress()
            from_func = fm.getFunctionContaining(from_a)
            fn = from_func.getName() if from_func else "?"
            fe = str(from_func.getEntryPoint()) if from_func else "?"
            ins = listing.getInstructionAt(from_a)
            fh.write("  from %s in %s @ %s: %s\n" % (
                from_a, fn, fe, ins.toString() if ins else "?"))
        fh.write("\n--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 90, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    print("wrote %s" % out)


# World-map-top overlay functions that call FUN_80034B78. These came from
# the find_jal_target.py sweep against overlay_world_map_top.bin.
OVERLAY_FUNCS = [
    0x801D0D38,  # 5 calls to FUN_80034B78 (first cluster)
    0x801D2EBC,  # 1 call
    0x801E5B4C,  # 9 calls (largest cluster - likely the per-tile loop)
    0x801E6400,  # 4 calls (continues from 801E5B4C)
]

# SCUS helpers worth pinning to understand the wrap chain.
SCUS_FUNCS = [
    0x80034B78,  # the heavily-called installer
]

# Try every address - dump_func_at silently skips ones not in this program.
for a in OVERLAY_FUNCS + SCUS_FUNCS:
    dump_func_at(a)

print("done")
