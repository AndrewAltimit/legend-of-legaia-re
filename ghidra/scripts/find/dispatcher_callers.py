# @category Legaia
# @runtime Jython
#
# Find callers of FUN_8001f05c (asset dispatcher) and FUN_8001a55c (LZS).
# For each caller, dump enough context to figure out where the descriptor
# bytes come from (which RAM region / which on-disc sector).

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

TARGETS = [
    ("FUN_8001f05c", 0x8001f05c),
    ("FUN_8001a55c", 0x8001a55c),
    ("FUN_8001a8b0", 0x8001a8b0),
]

callers_summary = {}

for tname, taddr in TARGETS:
    addr = af.getAddress("{:x}".format(taddr))
    refs = list(ref_mgr.getReferencesTo(addr))
    print("== {} @ 0x{:08X} : {} xrefs ==".format(tname, taddr, len(refs)))
    callers = {}
    for r in refs:
        from_a = r.getFromAddress()
        func = fm.getFunctionContaining(from_a)
        if func is None:
            continue
        fentry = func.getEntryPoint().getOffset()
        callers.setdefault(fentry, []).append(from_a)
    for fentry in sorted(callers):
        n = len(callers[fentry])
        sites = ", ".join(str(a) for a in callers[fentry][:5])
        if n > 5:
            sites += " ..."
        func = fm.getFunctionAt(af.getAddress("{:x}".format(fentry)))
        fname = func.getName() if func else "?"
        print("  {} @ 0x{:08X}  ({} call sites) [{}]".format(fname, fentry, n, sites))
        callers_summary.setdefault(fentry, set()).add(tname)
    print("")

print("== Functions calling MULTIPLE targets (likely orchestrators) ==")
for fentry, tnames in sorted(callers_summary.items()):
    if len(tnames) > 1:
        func = fm.getFunctionAt(af.getAddress("{:x}".format(fentry)))
        fname = func.getName() if func else "?"
        print("  {} @ 0x{:08X}  -> {}".format(fname, fentry, ", ".join(sorted(tnames))))
