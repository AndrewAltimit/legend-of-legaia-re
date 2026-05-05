# @category Legaia
# @runtime Jython
#
# Find writers + readers of the debug-flag RAM band 0x8007B400-0x8007BCFF.
# Documented occupants (cf. docs/EXTERNAL_RESEARCH.md):
#   0x8007B450 - debug-dispatch parameter (u16)
#   0x8007B6F4 - "Small maps" debug mode flag (u16)
#   0x8007B7C0 - debug-dispatch trigger / gate (u16)
#   0x8007B8C2 - dev/retail loader-path flag (u16, FUN_800255b8 branches on it)
#   0x8007B98F - in-game debug menu enable (byte, TCRF/Punk7890 GameShark)
#
# Two passes:
#   pass 1: ref-manager direct queries on each documented address
#   pass 2: LUI+offset scan over the whole range; reports stores and reads
#           per address with a per-function summary, sorted "stores first".
#
# We resolve effective addresses by tracking the most-recent lui per register
# within each function (resets at function boundary). Catches lui+addiu+sw
# combos that Ghidra's reference manager skips.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()

# Documented debug-region addresses worth a direct refmgr query.
DOCUMENTED = [0x8007B450, 0x8007B6F4, 0x8007B7C0, 0x8007B8C2, 0x8007B98F]
# Whole region for the LUI scan.
LO = 0x8007B400
HI = 0x8007BD00

# ---------------------------------------------------------------- pass 1 --
print("=" * 60)
print("PASS 1: ref-manager queries on documented addresses")
print("=" * 60)
for tgt in DOCUMENTED:
    addr = af.getAddress("{:x}".format(tgt))
    refs = list(ref_mgr.getReferencesTo(addr))
    print("\n== refs to 0x{:08X} -- {} ref(s) ==".format(tgt, len(refs)))
    if not refs:
        print("  (no direct refs from refmgr)")
        continue
    by_func = {}
    for r in refs:
        from_a = r.getFromAddress()
        rt = r.getReferenceType()
        func = fm.getFunctionContaining(from_a)
        fname = func.getName() if func else "?"
        fentry = str(func.getEntryPoint()) if func else "?"
        kind = "W" if rt.isWrite() else ("R" if rt.isRead() else "?")
        insn = listing.getInstructionAt(from_a)
        mnem = insn.getMnemonicString() if insn else "?"
        by_func.setdefault((fentry, fname), []).append((kind, str(from_a), mnem))
    for (fentry, fname), items in sorted(by_func.items()):
        ws = [it for it in items if it[0] == "W"]
        rs = [it for it in items if it[0] == "R"]
        print("  {} @ {}  W={} R={}".format(fname, fentry, len(ws), len(rs)))
        for k, ia, mn in ws[:6]:
            print("    [W] {}  {}".format(ia, mn))
        for k, ia, mn in rs[:3]:
            print("    [r] {}  {}".format(ia, mn))

# ---------------------------------------------------------------- pass 2 --
print("\n" + "=" * 60)
print("PASS 2: LUI+offset scan, range 0x{:08X}-0x{:08X}".format(LO, HI))
print("=" * 60)

# addr -> list of (kind, insn_addr_str, mnem, func_name, func_entry)
hits = {}

inst_iter = listing.getInstructions(True)
total = 0
last_lui = {}
current_func_addr = None

while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        last_lui = {}
        current_func_addr = fa
    mnem = insn.getMnemonicString()
    ops = insn.getNumOperands()

    if mnem == "lui" and ops == 2:
        try:
            reg = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except:
            pass
        continue

    if mnem == "addiu" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue()
            if src in last_lui:
                base = last_lui[src]
                last_lui[dst] = (base + imm) & 0xFFFFFFFF
        except:
            pass
        continue

    # `ori reg, src, imm` - low half load on top of an LUI also produces a constant
    if mnem == "ori" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue() & 0xFFFF
            if src in last_lui:
                base = last_lui[src]
                last_lui[dst] = (base | imm) & 0xFFFFFFFF
        except:
            pass
        continue

    if mnem in ("sw", "sh", "sb", "lw", "lh", "lhu", "lb", "lbu") and ops == 2:
        try:
            base_reg = insn.getDefaultOperandRepresentation(1)
            if "(" in base_reg and base_reg.endswith(")"):
                offstr, basestr = base_reg.split("(")
                basestr = basestr.rstrip(")")
                if offstr.startswith("0x"):
                    off = int(offstr, 16)
                elif offstr.startswith("-0x"):
                    off = -int(offstr[3:], 16)
                elif offstr and (offstr[0] == "-" or offstr[0].isdigit()):
                    off = int(offstr)
                else:
                    off = None
                if off is not None and basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if LO <= combined < HI:
                        kind = "W" if mnem.startswith("s") else "R"
                        fname = func.getName() if func else "?"
                        fentry = "0x{:08X}".format(fa) if fa else "?"
                        hits.setdefault(combined, []).append(
                            (kind, str(insn.getAddress()), mnem, fname, fentry))
        except:
            pass

print("\nscanned {} instructions, {} unique addresses hit".format(total, len(hits)))

# Summarize: addresses with stores first, then reads-only
addrs_with_writes = sorted([a for a, h in hits.items() if any(x[0] == "W" for x in h)])
addrs_read_only = sorted([a for a, h in hits.items() if not any(x[0] == "W" for x in h)])

print("\n--- ADDRESSES WITH AT LEAST ONE WRITE ({}) ---".format(len(addrs_with_writes)))
for a in addrs_with_writes:
    items = hits[a]
    by_func = {}
    for k, ia, mn, fn, fe in items:
        by_func.setdefault((fe, fn), []).append((k, ia, mn))
    print("\n  0x{:08X}: {} hits across {} fn(s)".format(a, len(items), len(by_func)))
    for (fe, fn), refs in sorted(by_func.items()):
        ws = [it for it in refs if it[0] == "W"]
        rs = [it for it in refs if it[0] == "R"]
        print("    {} @ {}  W={} R={}".format(fn, fe, len(ws), len(rs)))
        for k, ia, mn in ws[:8]:
            print("      [W] {}  {}".format(ia, mn))
        for k, ia, mn in rs[:4]:
            print("      [r] {}  {}".format(ia, mn))

print("\n--- READ-ONLY ADDRESSES ({}) ---".format(len(addrs_read_only)))
for a in addrs_read_only:
    items = hits[a]
    fns = sorted(set((it[4], it[3]) for it in items))
    print("  0x{:08X}: {} reads, {} fn(s): {}".format(
        a, len(items), len(fns),
        ", ".join("{}@{}".format(fn, fe) for fe, fn in fns[:4])))

print("\ndone")
