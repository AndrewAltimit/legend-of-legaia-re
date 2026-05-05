# @category Legaia
# @runtime Jython
prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()

TARGETS = [0x800204f8, 0x80020740, 0x80020224, 0x800204c8]

for tgt in TARGETS:
    addr = af.getAddress("{:x}".format(tgt))
    refs = list(ref_mgr.getReferencesTo(addr))
    print("\n== refs to FUN @ 0x{:08X} -- {} ref(s) ==".format(tgt, len(refs)))
    for r in refs:
        from_a = r.getFromAddress()
        rt = r.getReferenceType()
        func = fm.getFunctionContaining(from_a)
        fname = func.getName() if func else "?"
        fentry = str(func.getEntryPoint()) if func else "?"
        ins = listing.getInstructionAt(from_a)
        print("  from {} func {} @ {} kind {} insn {}".format(
            from_a, fname, fentry, rt, ins.toString() if ins else "?"))

print("\ndone")
