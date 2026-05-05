# @category Legaia
# @runtime Jython
#
# Find any instruction that writes to the global pointer at 0x8007b85c.
# That global holds the in-RAM asset table base; whoever writes it is
# the init function we want.
#
# We scan all instructions, look for sw/sh/sb whose target operand
# resolves to 0x8007b85c.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()

TARGETS = [0x8007b85c, 0x8007b888, 0x8007b828]  # the three globals seen in callers

for tgt in TARGETS:
    addr = af.getAddress("{:x}".format(tgt))
    print("\n== refs to 0x{:08X} ==".format(tgt))
    refs = list(ref_mgr.getReferencesTo(addr))
    if not refs:
        print("  (no direct refs from refmgr)")
    by_func = {}
    for r in refs:
        from_a = r.getFromAddress()
        rt = r.getReferenceType()
        func = fm.getFunctionContaining(from_a)
        fname = func.getName() if func else "?"
        fentry = str(func.getEntryPoint()) if func else "?"
        # is_write is true for WRITE refs
        is_write = rt.isWrite()
        is_read = rt.isRead()
        kind = "W" if is_write else ("R" if is_read else "?")
        insn = listing.getInstructionAt(from_a)
        mnem = insn.getMnemonicString() if insn else "?"
        print("  [{}] {} ({})  in {} @ {}  rt={}".format(
            kind, from_a, mnem, fname, fentry, rt))
        by_func.setdefault((fentry, fname), []).append((kind, str(from_a), mnem))
    print("  -- function summary --")
    for (fentry, fname), hits in by_func.items():
        kinds = "".join(h[0] for h in hits)
        print("    {} @ {}  [{}]  ({} refs)".format(fname, fentry, kinds, len(hits)))
