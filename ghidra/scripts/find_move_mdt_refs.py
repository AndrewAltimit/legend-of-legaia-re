# @category Legaia
# @runtime Jython
#
# Find xrefs to the dev string "\\move.mdt" in SCUS_942.54.
# String is at file offset 0x1150 in the EXE; PSX-EXE has 0x800-byte header
# and base addr 0x80010000, so RAM addr = 0x80010000 + (0x1150 - 0x800) = 0x80010950.

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
fm = prog.getFunctionManager()
mem = prog.getMemory()

needle_str = "\\move.mdt"

# debug: also probe several plausible RAM addresses
for probe in [0x80010950, 0x80010150, 0x80011150, 0x80010d40, 0x80010da0, 0x80011124]:
    try:
        a = af.getAddress("{:x}".format(probe))
        bs = []
        for j in range(16):
            bs.append("{:02x}".format(mem.getByte(a.add(j)) & 0xFF))
        ascii_repr = "".join(chr(int(b, 16)) if 0x20 <= int(b, 16) < 0x7F else "." for b in bs)
        print("probe 0x{:08X}: {} | {}".format(probe, " ".join(bs), ascii_repr))
    except Exception as e:
        print("probe 0x{:08X}: error {}".format(probe, e))

hits = [af.getAddress("80010950")]  # known location of "\move.mdt"
blocks = []  # disable block scan - already known
for b in blocks:
    if not b.isInitialized():
        continue
    name = b.getName()
    start = b.getStart()
    size = int(b.getSize())
    if size <= 0 or size > 0x300000:
        continue
    print("scanning block {} {} ({} bytes)".format(name, start, size))
    # bruteforce byte-by-byte
    bs = bytearray(size)
    b.getBytes(start, bs)
    raw_bytes = bytes(bs)
    needle_b = needle_str.encode("ascii")
    pos = 0
    while True:
        idx = raw_bytes.find(needle_b, pos)
        if idx < 0:
            break
        a = start.add(idx)
        hits.append(a)
        print("  found at:", a)
        pos = idx + 1

for h in hits:
    refs = list(ref_mgr.getReferencesTo(h))
    print("\nrefs to {}: {}".format(h, len(refs)))
    for r in refs:
        from_a = r.getFromAddress()
        func = fm.getFunctionContaining(from_a)
        fname = func.getName() if func else "?"
        fentry = str(func.getEntryPoint()) if func else "?"
        ins = listing.getInstructionAt(from_a)
        print("  from {} func {} @ {} insn: {}".format(
            from_a, fname, fentry, ins.toString() if ins else "?"))

print("\n--- LUI scan for ref to string addr ---")
for h in hits:
    target = h.getOffset()
    last_lui = {}
    cur_func = None
    inst_iter = listing.getInstructions(True)
    found = []
    while inst_iter.hasNext():
        insn = inst_iter.next()
        func = fm.getFunctionContaining(insn.getAddress())
        fa = func.getEntryPoint().getOffset() if func else None
        if fa != cur_func:
            last_lui = {}
            cur_func = fa
        mnem = insn.getMnemonicString()
        ops = insn.getNumOperands()
        if mnem == "lui" and ops == 2:
            try:
                reg = insn.getDefaultOperandRepresentation(0)
                imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
                last_lui[reg] = imm << 16
            except: pass
            continue
        if mnem == "addiu" and ops == 3:
            try:
                dst = insn.getDefaultOperandRepresentation(0)
                src = insn.getDefaultOperandRepresentation(1)
                imm = insn.getOpObjects(2)[0].getValue()
                if src in last_lui:
                    base = last_lui[src]
                    combined = (base + imm) & 0xFFFFFFFF
                    if combined == target:
                        fname = func.getName() if func else "?"
                        fe = "0x{:08X}".format(fa) if fa else "?"
                        found.append((str(insn.getAddress()), fname, fe))
                    last_lui[dst] = combined
            except: pass
            continue
    print("LUI-scan refs to {}: {}".format(h, len(found)))
    for ia, fn, fe in found:
        print("  {} {} @ {}".format(ia, fn, fe))

print("done")
