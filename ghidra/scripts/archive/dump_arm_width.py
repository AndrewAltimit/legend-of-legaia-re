# @runtime Jython
# @category Legaia
# Decompile the two functions that read the equipped weapon during arts INPUT
# (pinned by a live read-watch on the off-class Gala+Nail-Glove pre-build state):
#   0x801D3AD0  reads equip slot, conditionally +2 -> stores width to struct+0xe/+0xf
#   0x801ECC00  reads weapon slot for a jump-table dispatch (ra -> SCUS 0x800478A8)
# Output -> /scripts/funcs/arm_width_dump.txt
from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor
prog = getCurrentProgram(); fm = prog.getFunctionManager(); af = prog.getAddressFactory(); ref = prog.getReferenceManager()
def A(va): return af.getDefaultAddressSpace().getAddress(va)
dec = DecompInterface(); dec.openProgram(prog); mon = ConsoleTaskMonitor()
lines = []
def emit(s): lines.append(s)
def callers(va, lab):
    f = fm.getFunctionContaining(A(va))
    if not f: emit("[callers %s] no fn @0x%08x"%(lab,va)); return
    emit("[callers of %s = %s @0x%08x]:"%(lab, f.getName(), f.getEntryPoint().getOffset()))
    for r in ref.getReferencesTo(f.getEntryPoint()):
        cf = fm.getFunctionContaining(r.getFromAddress())
        emit("   from 0x%08x in %s"%(r.getFromAddress().getOffset(), cf.getName() if cf else "?"))
def deco(va, lab):
    f = fm.getFunctionContaining(A(va))
    emit("\n"+"="*72)
    if not f:
        f = createFunction(A(va), None)
    if not f: emit("[%s] no fn @0x%08x"%(lab,va)); return
    emit("[%s] %s entry=0x%08x (read site 0x%08x)"%(lab, f.getName(), f.getEntryPoint().getOffset(), va))
    r = dec.decompileFunction(f, 60, mon)
    emit(r.getDecompiledFunction().getC() if r and r.decompileCompleted() else "<failed>")
for va,lab in [(0x801D3AD0,"width_plus2"),(0x801ECC00,"weapon_dispatch")]:
    callers(va,lab); deco(va,lab)
open("/scripts/funcs/arm_width_dump.txt","w").write("\n".join(lines))
print("wrote arm_width_dump.txt", len(lines))
