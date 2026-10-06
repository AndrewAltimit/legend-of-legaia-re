# @category Legaia
# @runtime Jython
#
# C1 dump-worklist stragglers: cited-in-docs functions with no dump file.
# Run against the field-overlay programs; the in_program() guard skips
# addresses outside the current program, and output is prefixed with the
# program label so VA-aliased overlays stay distinguishable:
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_cutscene_dialogue.bin -noanalysis \
#       -postScript /scripts/dump_c1_stragglers.py
#   (repeat with -process overlay_0897_xxx_dat.bin etc.)

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801e0c3c",  # field-VM 0x4C high-nibble re-dispatch (JT 0x801CEE60)
    "801f747c",  # tutorial box helper (str, mode)
    "801f7628",  # tutorial step-2 (spirit lesson) sub-emitter
    "801c9688",  # world-map: reads + clears the walk-camera handoff cell
    "801db0f0",  # turn-resolution commit helper (FUN_801F138C trio)
    "801e5154",  # field-overlay effect-descriptor handler (0x801F291C slots)
]

OUT_DIR = "/scripts/funcs"


def in_program(addr_str):
    a = currentProgram.getAddressFactory().getAddress(addr_str)
    return currentProgram.getMemory().contains(a)


def out_path_for(addr_str):
    label = currentProgram.getName().replace(".bin", "")
    return os.path.join(OUT_DIR, "{}_{}.txt".format(label, addr_str))


decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(currentProgram)
monitor = ConsoleTaskMonitor()
listing = currentProgram.getListing()

for t in TARGETS:
    addr_str = "0x" + t
    if not in_program(addr_str):
        print("skip {} (not in {})".format(t, currentProgram.getName()))
        continue
    addr = currentProgram.getAddressFactory().getAddress(addr_str)
    func = currentProgram.getFunctionManager().getFunctionContaining(addr)
    if func is None:
        print("no function at {}".format(t))
        continue
    out_path = out_path_for(t)
    fh = open(out_path, "w")
    fh.write("-- {} in {} (entry {})\n\n".format(t, currentProgram.getName(), func.getEntryPoint()))
    fh.write("--- DISASSEMBLY ---\n\n")
    ins = listing.getInstructions(func.getBody(), True)
    while ins.hasNext():
        i = ins.next()
        fh.write("{}  {}\n".format(i.getAddress(), i))
    fh.write("\n--- DECOMPILED ---\n\n")
    try:
        res = decomp.decompileFunction(func, 60, monitor)
        if res.decompileCompleted():
            fh.write(res.getDecompiledFunction().getC())
        else:
            fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
    except Exception as e:
        fh.write("(decompile exception: {})\n".format(e))
    fh.close()
    print("wrote {}".format(out_path))
print("done")
