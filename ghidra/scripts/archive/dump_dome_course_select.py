# dump_dome_course_select.py
#
# @category Legaia
# @runtime Jython
#
# Dumps the Muscle Dome arena course-SELECT init and its neighbours from the
# PROT 0977 arena overlay at its true slot-A base 0x801CE818. FUN_801CEA6C
# seeds the packed course/round word _DAT_8007BAC0 from the course-unlock
# story flags (0x536/0x537/0x538 -> courses 0/1/2); the Delilas Challenge
# re-architecture needs its seed chain to add a 4th course.
#
# These addresses are only correct in the 0977 program (VA-aliased under the
# 0897 town overlay at the same load base); dumping from 0897 gives a
# different function's bytes.
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process 0977_other_game.BIN -noanalysis \
#     -postScript /scripts/dump_dome_course_select.py

import os
from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

PROGRAM_TARGETS = {
    "0977_other_game.BIN": ("overlay_0977_slotA", [
        "801cea6c",  # course-select init (seed chain: flags 0x536/537/538)
        "801ce818",  # overlay entry / dispatch head
        "801d0f60",  # contest settlement + prize (reward hook target)
    ]),
}

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
decomp.openProgram(prog)

entry = PROGRAM_TARGETS.get(prog_name)
if entry is None:
    print("[dump] program %s not in target map; skipping" % prog_name)
else:
    label, targets = entry
    for va in targets:
        addr = af.getAddress(va)
        if not mem.contains(addr):
            print("[dump] %s not in program %s; skip" % (va, prog_name))
            continue
        out_path = "%s/%s_%s.txt" % (OUT_DIR, label, va)
        fn = fm.getFunctionContaining(addr)
        with open(out_path, "w") as f:
            f.write("== dump target 0x%s (program %s) ==\n" % (va, prog_name))
            if fn is not None:
                f.write("enclosing function: %s @ %s\n\n" % (
                    fn.getName(), fn.getEntryPoint()))
                body = fn.getBody()
                f.write("--- DISASSEMBLY ---\n")
                ci = listing.getInstructions(body, True)
                for ins in ci:
                    f.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
                f.write("\n--- DECOMPILED ---\n")
                res = decomp.decompileFunction(fn, 60, monitor)
                if res.decompileCompleted():
                    f.write(res.getDecompiledFunction().getC())
                else:
                    f.write("(decompile failed)\n")
            else:
                f.write("(no function contains this address; raw window)\n")
                a = addr
                for _ in range(80):
                    ins = listing.getInstructionAt(a)
                    if ins is None:
                        break
                    f.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
                    a = ins.getAddress().add(ins.getLength())
        print("[dump] wrote %s" % out_path)

print("[dump] done")
