# @runtime Jython
# @category Legaia
#
# Dump every function in the currently-loaded summon overlay program (PROT 905,
# imported as raw MIPS at base 0x801F0000) to per-function decompiled-C files
# under /scripts/funcs/summon905_<addr>.txt. Flags functions that reference the
# record table (0x801F180C) or call the part-stager FUN_80021B04 (0x80021B04),
# so the staging logic is easy to find. ASCII-only (Jython chokes on Unicode).

import os

from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

TABLE_ADDR = 0x801F180C
STAGER_CALL = 0x80021B04

OUT_DIR = "/scripts/funcs"
prog = currentProgram
listing = prog.getListing()
fm = prog.getFunctionManager()

decomp = DecompInterface()
decomp.openProgram(prog)
monitor = ConsoleTaskMonitor()

flagged = []
count = 0
for func in fm.getFunctions(True):
    entry = func.getEntryPoint()
    addr_int = entry.getOffset()
    # Scan the function's instructions for the table ref / stager call.
    refs_table = False
    calls_stager = False
    body = func.getBody()
    instrs = listing.getInstructions(body, True)
    for ins in instrs:
        mnem = ins.getMnemonicString()
        for opi in range(ins.getNumOperands()):
            for ref in ins.getOperandReferences(opi):
                tgt = ref.getToAddress().getOffset()
                if tgt == TABLE_ADDR:
                    refs_table = True
                if tgt == STAGER_CALL:
                    calls_stager = True
        # MIPS jal encodes target as a flow ref too; also check scalar operands.
        for opi in range(ins.getNumOperands()):
            obj = ins.getOpObjects(opi)
            for o in obj:
                try:
                    v = o.getValue()
                    if v == TABLE_ADDR:
                        refs_table = True
                    if v == STAGER_CALL and mnem.lower().startswith("jal"):
                        calls_stager = True
                except Exception:
                    pass

    res = decomp.decompileFunction(func, 60, monitor)
    c = res.getDecompiledFunction().getC() if res.decompileCompleted() else "/* decompile failed */"

    tag = ""
    if refs_table:
        tag += " TABLE_REF"
    if calls_stager:
        tag += " STAGER_CALL"
    if tag:
        flagged.append("0x%08X%s" % (addr_int, tag))

    path = os.path.join(OUT_DIR, "summon905_%08x.txt" % addr_int)
    f = open(path, "w")
    f.write("== summon905 %08x (entry=%08x)%s ==\n\n" % (addr_int, addr_int, tag))
    f.write(c)
    f.write("\n\n--- DISASSEMBLY ---\n")
    for ins in listing.getInstructions(body, True):
        f.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
    f.close()
    count += 1

print("dumped %d functions" % count)
print("FLAGGED (staging candidates):")
for fl in flagged:
    print("  " + fl)
