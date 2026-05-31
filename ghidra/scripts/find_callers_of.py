# @category Legaia
# @runtime Jython
#
# Generic "find callers of one or more target functions" helper.
# Targets edited inline; outputs caller list with the call-site instruction.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

# Edit this list to point at the function entry points you want callers for.
TARGETS_HEX = [
    "801d71f0",
    "801d7210",
]

for t in TARGETS_HEX:
    addr = af.getAddress(t)
    refs = list(ref_mgr.getReferencesTo(addr))
    target_fn = fm.getFunctionAt(addr)
    name = target_fn.getName() if target_fn else "?"
    print("\n=== {} ({}) -- {} refs ===".format(t, name, len(refs)))
    for r in refs:
        from_a = r.getFromAddress()
        from_func = fm.getFunctionContaining(from_a)
        from_fn_name = from_func.getName() if from_func else "?"
        from_fn_entry = str(from_func.getEntryPoint()) if from_func else "?"
        ins = listing.getInstructionAt(from_a)
        print("  from {} in {} @ {}: {}".format(
            from_a, from_fn_name, from_fn_entry,
            ins.toString() if ins else "?"))
