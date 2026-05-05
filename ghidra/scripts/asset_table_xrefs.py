# @category Legaia
# @runtime Jython
#
# Find xrefs to and around 0x801C70F0 (the in-RAM asset table).
# We scan a small range because the table is likely an array; refs into it
# may use base+offset addressing (lui + addiu pairs land on the LUI page).

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

# Probe addresses: the table itself, and a few likely-stride offsets.
# If entries are 4 or 8 bytes, refs into other entries will hit these.
PROBES = [
    0x801C70F0,
    0x801C70F4,
    0x801C70F8,
    0x801C70FC,
    0x801C7100,
    0x801C7104,
    0x801C7108,
    0x801C7110,
    0x801C7120,
    0x801C7140,
    0x801C7180,
    0x801C7200,
]

print("== Direct xrefs to asset-table probe addresses ==")
all_callers = {}
for p in PROBES:
    addr = af.getAddress("{:x}".format(p))
    refs = list(ref_mgr.getReferencesTo(addr))
    if not refs:
        continue
    print("\n  0x{:08X}  ({} refs)".format(p, len(refs)))
    for r in refs:
        from_a = r.getFromAddress()
        func = fm.getFunctionContaining(from_a)
        fname = func.getName() if func else "<no func>"
        fentry = str(func.getEntryPoint()) if func else "?"
        rt = r.getReferenceType()
        print("    {}  type={}  func={} @ {}".format(from_a, rt, fname, fentry))
        if func is not None:
            key = (fentry, fname)
            all_callers[key] = all_callers.get(key, 0) + 1

print("\n== Unique functions touching the table region ==")
for (fentry, fname), n in sorted(all_callers.items(), key=lambda kv: -kv[1]):
    print("  {} @ {}  ({} refs)".format(fname, fentry, n))

# Also scan instructions for any operand that resolves into the page.
# This catches lui+addiu/lw pairs whose computed address Ghidra has annotated.
print("\n== Scanning all instructions for operand refs into 0x801C7000-0x801C7400 ==")
LO = 0x801C7000
HI = 0x801C7400
hit_funcs = {}
inst_iter = listing.getInstructions(True)
total = 0
while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    for ref in insn.getReferencesFrom():
        ta = ref.getToAddress()
        if ta is None:
            continue
        try:
            v = ta.getOffset()
        except:
            continue
        if LO <= v < HI:
            ia = insn.getAddress()
            func = fm.getFunctionContaining(ia)
            fname = func.getName() if func else "<no func>"
            fentry = str(func.getEntryPoint()) if func else "?"
            key = (fentry, fname)
            if key not in hit_funcs:
                hit_funcs[key] = []
            hit_funcs[key].append((str(ia), "0x{:08X}".format(v), str(ref.getReferenceType())))

print("scanned {} instructions".format(total))
print("functions with refs into table region: {}".format(len(hit_funcs)))
for (fentry, fname), hits in sorted(hit_funcs.items(), key=lambda kv: -len(kv[1])):
    print("\n  {} @ {}  ({} refs)".format(fname, fentry, len(hits)))
    for ia, target, rt in hits[:10]:
        print("    {}  --> {}  ({})".format(ia, target, rt))
    if len(hits) > 10:
        print("    ... +{} more".format(len(hits) - 10))
