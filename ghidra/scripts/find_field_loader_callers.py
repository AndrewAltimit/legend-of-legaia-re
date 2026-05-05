# @category Legaia
# @runtime Jython
#
# Find callers of the field/town asset loaders FUN_8001f7c0 and FUN_800255b8
# and dump the surrounding instructions before each call site so we can see
# where the scene-name argument comes from.
#
# Output one block per target, listing each caller function and the 12
# instructions before the call (capturing argument-prep LUI/ADDIU/JAL chains).

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

TARGETS = [
    ("FUN_8001f7c0", 0x8001f7c0),  # field stage loader (.MAP/.PCH/efect.dat)
    ("FUN_800255b8", 0x800255b8),  # per-asset-type field loader
    ("FUN_8002541c", 0x8002541c),  # streaming-asset driver (calls 800255b8)
]

CONTEXT_BEFORE = 12  # instructions before each call to dump


def fmt_ins(ins):
    return "{}  {}".format(ins.getAddress(), ins.toString())


def context_before(call_addr, n):
    """Return up to `n` instructions immediately preceding `call_addr`."""
    out = []
    cursor = listing.getInstructionBefore(call_addr)
    while cursor is not None and len(out) < n:
        out.append(cursor)
        cursor = listing.getInstructionBefore(cursor.getAddress())
    out.reverse()
    return out


for tname, taddr in TARGETS:
    addr = af.getAddress("{:x}".format(taddr))
    refs = list(ref_mgr.getReferencesTo(addr))
    print("\n== {} @ 0x{:08X} : {} xrefs ==".format(tname, taddr, len(refs)))
    by_func = {}
    for r in refs:
        from_a = r.getFromAddress()
        func = fm.getFunctionContaining(from_a)
        if func is None:
            continue
        fentry = func.getEntryPoint().getOffset()
        by_func.setdefault(fentry, []).append(from_a)
    for fentry in sorted(by_func):
        sites = by_func[fentry]
        func = fm.getFunctionAt(af.getAddress("{:x}".format(fentry)))
        fname = func.getName() if func else "?"
        print("\n  caller: {} @ 0x{:08X}  ({} call sites)".format(fname, fentry, len(sites)))
        for site in sites:
            print("    site: {}".format(site))
            ctx = context_before(site, CONTEXT_BEFORE)
            for ins in ctx:
                print("      {}".format(fmt_ins(ins)))
