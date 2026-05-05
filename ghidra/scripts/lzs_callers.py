# @category Legaia
# @runtime Jython
#
# Lists every call site of FUN_8001a55c (the LZS decompressor).

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()

target = af.getAddress("8001a55c")
refs = list(ref_mgr.getReferencesTo(target))
print("xrefs to FUN_8001a55c: {}".format(len(refs)))
seen = set()
for r in refs:
    addr = r.getFromAddress()
    func = fm.getFunctionContaining(addr)
    fname = func.getName() if func is not None else "<no func>"
    fentry = str(func.getEntryPoint()) if func is not None else "?"
    seen.add((fentry, fname))
    print("  call from {}  in {} @ {}".format(addr, fname, fentry))

print("\nunique calling functions: {}".format(len(seen)))
