# @category Legaia
# @runtime Jython
#
# Find all readers and writers of DAT_8007b7c8 (the ANM buffer pointer set
# by FUN_8001f05c case 6).
#
# Strategy: the address 0x8007B7C8 is loaded via lui+(load|store) pairs.
# In MIPS the displacement form is `(lw|sw|...) reg, -0x4838(base)` after
# `lui base, 0x8008` (since 0x8007B7C8 - 0x80080000 = -0x4838).
#
# We track the most recent lui per register within each function and emit
# any load/store whose computed effective address lies in a small window
# around the ANM globals.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# Window: just the ANM pointer + a few bytes either side, so we don't drown
# in unrelated globals.
TARGET = 0x8007B7C8
LO = TARGET - 0x10
HI = TARGET + 0x20

inst_iter = listing.getInstructions(True)
total = 0
hits = {}  # function entry -> list of (addr, kind, target)

last_lui = {}  # reg -> hi_value (per current function)
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
                combined = (base + imm) & 0xFFFFFFFF
                if LO <= combined < HI:
                    hits.setdefault(fa, []).append(
                        (str(insn.getAddress()), "lui+addiu", "0x{:08X}".format(combined))
                    )
                last_lui[dst] = combined
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
                else:
                    off = int(offstr, 0)
                if basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if LO <= combined < HI:
                        kind = "STORE" if mnem.startswith("s") else "load"
                        hits.setdefault(fa, []).append(
                            (str(insn.getAddress()), "{} ({})".format(mnem, kind), "0x{:08X}".format(combined))
                        )
        except:
            pass

print("scanned {} instructions".format(total))
print("functions touching 0x{:08X}-0x{:08X}: {}".format(LO, HI, len(hits)))
for fa, refs in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa))) if fa else None
    fname = func.getName() if func else "?"
    stores = [r for r in refs if "STORE" in r[1]]
    loads = [r for r in refs if "load" in r[1]]
    addrs = [r for r in refs if "lui+addiu" in r[1]]
    print("\n  {} @ 0x{:08X}  stores={} loads={} addrs={}".format(
        fname, fa or 0, len(stores), len(loads), len(addrs)))
    for ia, kind, tgt in stores[:8]:
        print("    [W] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in loads[:8]:
        print("    [r] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in addrs[:4]:
        print("    [a] {}  {}  {}".format(ia, kind, tgt))
