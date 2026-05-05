# @category Legaia
# @runtime Jython
#
# Debug helper: dump every jal/jalr in the overlay program with target.

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()

probe_addr = af.getAddress("801dd9a8")
ins = listing.getInstructionAt(probe_addr)
print("@801dd9a8: instruction = {!r}".format(ins))
if ins is not None:
    print("  mnem={}, num_ops={}".format(ins.getMnemonicString(), ins.getNumOperands()))
    for i in range(ins.getNumOperands()):
        objs = ins.getOpObjects(i)
        print("  op{}: {!r} -> {!r}".format(i, objs, [type(o).__name__ for o in objs]))
        for o in objs:
            if hasattr(o, "getOffset"):
                print("    offset = 0x{:X}".format(o.getOffset()))
    refs = ins.getReferencesFrom()
    print("  references_from = {}".format([str(r) for r in refs]))

# Count total jal instructions
ins_iter = listing.getInstructions(True)
jal_count = 0
loader_calls = {}
LOADERS = {
    0x8003E8A8, 0x8003EB98, 0x8003E6BC, 0x8003E800,
    0x800520F0, 0x8001F7C0, 0x8001E890, 0x8001ED60,
}
for ins in ins_iter:
    mnem = ins.getMnemonicString().lower()
    if mnem in ("jal", "jalr"):
        jal_count += 1
        objs = ins.getOpObjects(0)
        for o in objs:
            if hasattr(o, "getOffset"):
                tgt = o.getOffset() & 0xFFFFFFFF
                if tgt in LOADERS:
                    loader_calls.setdefault(tgt, []).append(str(ins.getAddress()))
print("\ntotal jal/jalr in overlay program: {}".format(jal_count))
print("loader-target hits:")
for tgt, sites in loader_calls.items():
    print("  0x{:X}: {} sites".format(tgt, len(sites)))
    for s in sites[:5]:
        print("    {}".format(s))
