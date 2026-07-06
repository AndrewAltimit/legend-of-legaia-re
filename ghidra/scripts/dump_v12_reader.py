# @category Legaia
# @runtime Jython
#
# Wave-8 Arc 6: the scene-v12 record-table consumer lead. The reader of
# _DAT_8007b85c at ~0x800219xx sits in un-analyzed space (no function, no
# disassembly - invisible to instruction walks). Raw-byte prologue scan
# pins the entry at 0x80021940 (lui/lw pairs at 0x800219dc/0x800219e0 and
# 0x80021ac8/0x80021acc, jr ra at 0x80021afc). Force disassembly, create
# the function, dump disasm + decomp to /scripts/funcs/80021940.txt.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.program.model.address import AddressSet

ENTRY = 0x80021940

prog = currentProgram
af = prog.getAddressFactory().getDefaultAddressSpace()
fm = prog.getFunctionManager()
listing = prog.getListing()
monitor = ConsoleTaskMonitor()

entry = af.getAddress(ENTRY)

# Force disassembly if the region is undefined.
if listing.getInstructionAt(entry) is None:
    disassemble(entry)

func = fm.getFunctionContaining(entry)
if func is None:
    func = createFunction(entry, "FUN_%08x" % ENTRY)
if func is None:
    func = fm.getFunctionContaining(entry)

if func is None:
    print("FAILED to create function at %08x" % ENTRY)
else:
    ep = func.getEntryPoint()
    body = func.getBody()
    lines = []
    lines.append("== %s %s (entry=%s) ==" % (func.getName(), ep, ep))
    lines.append("size=%d bytes" % body.getNumAddresses())
    lines.append("")
    lines.append("--- DISASSEMBLY ---")
    ii = listing.getInstructions(body, True)
    while ii.hasNext():
        insn = ii.next()
        lines.append("%s  %s" % (insn.getAddress(), insn))
    lines.append("")
    lines.append("--- DECOMPILED ---")
    ifc = DecompInterface()
    ifc.setOptions(DecompileOptions())
    ifc.openProgram(prog)
    res = ifc.decompileFunction(func, 120, monitor)
    if res is not None and res.decompileCompleted():
        lines.append(res.getDecompiledFunction().getC())
    else:
        lines.append("(decompile failed)")
    out = "/scripts/funcs/%08x.txt" % ENTRY
    f = open(out, "w")
    f.write("\n".join(lines))
    f.close()
    print("dumped %s (%d lines)" % (out, len(lines)))
