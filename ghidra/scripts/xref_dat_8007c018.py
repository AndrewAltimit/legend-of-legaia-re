# @category Legaia
# @runtime Jython
#
# Query Ghidra's reference manager for everything that references the
# DAT_8007C018 table window. This catches references that lui+addiu
# pattern-matchers miss (and avoids false positives from def-use clobbers).
#
# Also dumps the raw word values at each window slot - if the program is
# SCUS_942.54 they will be 0 (BSS), but reading them in an overlay context
# can surface any constants that got snapshotted to disk.
#
# Reports writes / reads / data references in the window 0x8007C000 ..
# 0x8007C400.

from ghidra.program.model.symbol import RefType

prog = currentProgram
prog_name = prog.getName()
ref_mgr = prog.getReferenceManager()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
mem = prog.getMemory()
listing = prog.getListing()

LO = 0x8007C000
HI = 0x8007C400

print("=== %s ===" % prog_name)

writers = []
readers = []
data_refs = []

cur = LO
while cur < HI:
    target = af.getAddress("%x" % cur)
    refs = ref_mgr.getReferencesTo(target)
    for ref in refs:
        rt = ref.getReferenceType()
        from_addr = ref.getFromAddress()
        func = fm.getFunctionContaining(from_addr)
        func_name = func.getName() if func else "?"
        entry = "0x%X  %s @ %s  -> 0x%08X  (%s)" % (
            from_addr.getOffset(), func_name, from_addr, cur, rt)
        if rt.isWrite():
            writers.append(entry)
        elif rt.isRead():
            readers.append(entry)
        else:
            data_refs.append(entry)
    cur += 4

print("\nWRITES to 0x%08X..0x%08X  (%d)" % (LO, HI, len(writers)))
for w in writers:
    print("  " + w)
print("\nREADS (%d) [first 10]" % len(readers))
for r in readers[:10]:
    print("  " + r)
print("\nDATA refs / unknown (%d) [first 10]" % len(data_refs))
for d in data_refs[:10]:
    print("  " + d)
