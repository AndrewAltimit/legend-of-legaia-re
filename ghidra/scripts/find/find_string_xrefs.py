# @category Legaia
# @runtime Jython
#
# Find dev-path strings in SCUS_942.54 (e.g. "h:\prot\..." paths) and dump
# every code site that references them. The dev paths are gold for
# locating asset-loader functions: the engine calls a path-resolver with
# a string literal arg, and grep'ing for the string in disassembly tells
# you exactly which function loads what.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()
mem = prog.getMemory()

PATH_NEEDLES = [
    "h:\\prot\\all\\data\\field\\player.lzs",
    "h:\\prot\\battle\\etim.dat",
    "h:\\prot\\battle\\etmd.dat",
    "h:\\prot\\battle\\vdf.dat",
    "h:\\prot\\field\\card\\tim.dat",
    "h:\\PROT\\FIELD\\",
    "h:\\prot\\all\\mapname",
    "h:\\prot\\cdname.dat",
    "h:\\prot\\cdname.txt",
]


def find_string_addrs(needle):
    """Scan defined strings in the program for matches on `needle`.
    Returns a list of (addr, value) tuples."""
    hits = []
    di = listing.getDefinedData(True)
    while di.hasNext():
        d = di.next()
        if d is None:
            continue
        try:
            v = d.getValue()
        except Exception:
            continue
        if v is None:
            continue
        s = str(v)
        if needle.lower() in s.lower():
            hits.append((d.getAddress(), s))
    return hits


print("== Dev-path xrefs in SCUS_942.54 ==")
for needle in PATH_NEEDLES:
    matches = find_string_addrs(needle)
    if not matches:
        print("\n[ {} ]  (no string found)".format(needle))
        continue
    for addr, s in matches:
        refs = list(ref_mgr.getReferencesTo(addr))
        print("\n[ {!r} @ {} ]  refs={}".format(s, addr, len(refs)))
        for r in refs:
            from_a = r.getFromAddress()
            func = fm.getFunctionContaining(from_a)
            fname = func.getName() if func else "<no func>"
            fentry = str(func.getEntryPoint()) if func else "?"
            print("    {}  in {} @ {}".format(from_a, fname, fentry))
