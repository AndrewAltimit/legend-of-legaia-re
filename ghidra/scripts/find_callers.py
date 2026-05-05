# @category Legaia
# @runtime Jython
#
# Find xrefs to a target function and dump caller funcs + the surrounding
# 8 instructions before each call site (to capture the argument prep).

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

TARGETS = [
    ("FUN_80020224", 0x80020224),  # the (type_size, data_offset) walker
    ("FUN_800198e0", 0x800198e0),  # the offset-table item handler
    ("FUN_80017888", 0x80017888),  # malloc
    ("FUN_8003e4e8", 0x8003e4e8),  # boot-time TOC loader candidate
    ("FUN_8005e9a4", 0x8005e9a4),  # CD-read primitive used by 8003e4e8
    ("FUN_8005dbb4", 0x8005dbb4),  # directory lookup used by 8003e4e8
]

for tname, taddr in TARGETS:
    addr = af.getAddress("{:x}".format(taddr))
    refs = list(ref_mgr.getReferencesTo(addr))
    print("\n== {} @ 0x{:08X} : {} xrefs ==".format(tname, taddr, len(refs)))
    callers = {}
    for r in refs:
        from_a = r.getFromAddress()
        func = fm.getFunctionContaining(from_a)
        if func is None:
            continue
        fentry = func.getEntryPoint().getOffset()
        callers.setdefault(fentry, []).append(from_a)
    for fentry in sorted(callers):
        sites = callers[fentry]
        func = fm.getFunctionAt(af.getAddress("{:x}".format(fentry)))
        fname = func.getName() if func else "?"
        print("  {} @ 0x{:08X}  ({} call sites)".format(fname, fentry, len(sites)))
        for site in sites[:3]:
            print("    site: {}".format(site))
