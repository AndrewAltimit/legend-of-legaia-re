# @category Legaia
# @runtime Jython
#
# Find consumers (readers) of the MOVE (case 5) and MOVE2 (case 0xB) buffers
# allocated by FUN_8001f05c (the asset dispatcher). These pointers hold the
# move_program_no PROT entries (Tactical Arts move definition tables).
#
# Targets:
#   0x8007B888  -- MOVE buffer pointer  (set by FUN_8001f05c case 5)
#   0x8007B840  -- MOVE2 buffer pointer (set by FUN_8001f05c case 0xB)
#
# Two passes:
#   pass 1: ref-manager direct queries
#   pass 2: LUI+offset scan over the whole binary; reports stores AND reads,
#           per-function summary.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()

TARGETS = [0x8007B888, 0x8007B840]

# ---------------------------------------------------------------- pass 1 --
print("=" * 60)
print("PASS 1: ref-manager queries")
print("=" * 60)
for tgt in TARGETS:
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
        for k, ia, mn in rs[:6]:
            print("    [r] {}  {}".format(ia, mn))

# ---------------------------------------------------------------- pass 2 --
print("\n" + "=" * 60)
print("PASS 2: LUI+offset scan for both targets")
print("=" * 60)

target_set = set(TARGETS)
hits = {a: [] for a in TARGETS}

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
                    if combined in target_set:
                        kind = "W" if mnem.startswith("s") else "R"
                        fname = func.getName() if func else "?"
                        fentry = "0x{:08X}".format(fa) if fa else "?"
                        hits[combined].append(
                            (kind, str(insn.getAddress()), mnem, fname, fentry))
        except:
            pass

print("\nscanned {} instructions".format(total))

for a in TARGETS:
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
        for k, ia, mn in rs[:8]:
            print("      [r] {}  {}".format(ia, mn))

print("\ndone")
