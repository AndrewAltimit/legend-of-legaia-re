# @runtime Jython
# @category Legaia
#
# Decompile the battle-overlay arts-combo input/execution cluster to locate the
# arm-width (off-class weapon -> arm consumes 2 gauge spaces) consumer + its data.
# Targets: the Arms execution resolver FUN_801EC3E4 (cursor actor+0x1f4, capacity
# combo+0x10, power table 0x801F64E4), its callers, and functions referencing the
# move-power table 0x801F4F5C. Output -> /scripts/funcs/arts_input_dump.txt
import os
from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

prog = getCurrentProgram()
fm = prog.getFunctionManager()
af = prog.getAddressFactory()
ref = prog.getReferenceManager()
listing = prog.getListing()

OUT = "/scripts/funcs/arts_input_dump.txt"
lines = []
def emit(s): lines.append(s)

def addr(va): return af.getDefaultAddressSpace().getAddress(va)

dec = DecompInterface()
dec.openProgram(prog)
mon = ConsoleTaskMonitor()

def decompile(va, label):
    a = addr(va)
    f = fm.getFunctionContaining(a)
    emit("\n" + "="*72)
    if f is None:
        emit("[%s] 0x%08x : NO FUNCTION (creating)" % (label, va))
        f = createFunction(a, None)
        if f is None:
            emit("  could not create function here"); return None
    emit("[%s] %s @ 0x%08x  (entry 0x%08x)" % (label, f.getName(), va, f.getEntryPoint().getOffset()))
    res = dec.decompileFunction(f, 60, mon)
    if res and res.decompileCompleted():
        emit(res.getDecompiledFunction().getC())
    else:
        emit("  <decompile failed>")
    return f

def callers_of(va, label):
    a = addr(va)
    f = fm.getFunctionContaining(a)
    if f is None:
        emit("[callers %s] no function at 0x%08x" % (label, va)); return []
    ent = f.getEntryPoint()
    cs = []
    it = ref.getReferencesTo(ent)
    for r in it:
        frm = r.getFromAddress()
        cf = fm.getFunctionContaining(frm)
        cs.append((frm.getOffset(), cf.getEntryPoint().getOffset() if cf else 0, cf.getName() if cf else "?"))
    emit("[callers of %s @0x%08x] %d refs:" % (label, ent.getOffset(), len(cs)))
    for frm, cfe, nm in cs:
        emit("   from 0x%08x  in %s (0x%08x)" % (frm, nm, cfe))
    return cs

# 1) the Arms execution resolver
RESOLVER = 0x801EC3E4
callers_of(RESOLVER, "resolver")
decompile(RESOLVER, "ArmsResolver")

# 2) anything that references the move-power table 0x801F4F5C / power byte 0x801F64E4
emit("\n" + "#"*72)
emit("# functions referencing move-power 0x801F4F5C / power 0x801F64E4")
seen = set()
for tva in (0x801F4F5C, 0x801F64E4, 0x801F4E63):
    for r in ref.getReferencesTo(addr(tva)):
        frm = r.getFromAddress()
        cf = fm.getFunctionContaining(frm)
        if cf:
            e = cf.getEntryPoint().getOffset()
            if e not in seen:
                seen.add(e)
                emit("  0x%08x %s  (ref->0x%08x from 0x%08x)" % (e, cf.getName(), tva, frm.getOffset()))

# 3) decompile each distinct referencing function (the arts execution / input cluster)
for e in sorted(seen):
    decompile(e, "movepower-ref")

with open(OUT, "w") as fh:
    fh.write("\n".join(lines))
print("wrote", OUT, len(lines), "lines")
