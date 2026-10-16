# @category Legaia
# @runtime Jython
#
# Generic LUI + ADDIU/load/store resolver. Walks every instruction, tracks
# the most-recent `lui R, hi` per register (resets at function boundaries),
# and reports any subsequent `addiu R, R, lo` or memory access whose
# effective address falls in the [LO, HI] window. Ghidra's reference
# manager misses these combined accesses, especially store-base + offset
# combos.
#
# Edit LO / HI below to point at the address range you're tracing.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# We're looking for stores whose effective address falls in this range.
LO = 0x801C7000
HI = 0x801CB000  # generous

# Track recent lui per register so we can resolve combined addresses.
# Walk all instructions in order.
hits = {}  # function entry -> list of (addr, kind, target)

inst_iter = listing.getInstructions(True)
total = 0
# Per-function lui state
last_lui = {}  # reg -> (addr, hi_value)
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
    # addiu reg, src_reg, imm  -> compute combined value
    if mnem == "addiu" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue()
            if src in last_lui:
                base = last_lui[src]
                combined = (base + imm) & 0xFFFFFFFF
                if LO <= combined < HI:
                    hits.setdefault(fa, []).append((str(insn.getAddress()), "lui+addiu", "0x{:08X}".format(combined)))
                last_lui[dst] = combined
        except:
            pass
        continue
    # sw/sb/sh reg, off(base) - store
    if mnem in ("sw", "sh", "sb", "lw", "lh", "lhu", "lb", "lbu") and ops == 2:
        try:
            # operand 1 is "off(base)" form
            base_reg = insn.getDefaultOperandRepresentation(1)
            # parse "off(base)"
            if "(" in base_reg and base_reg.endswith(")"):
                offstr, basestr = base_reg.split("(")
                basestr = basestr.rstrip(")")
                off = int(offstr, 16) if offstr.startswith("0x") else int(offstr, 0) if offstr.startswith("-0x") or offstr[0] in "-+0123456789" else None
                if off is not None and basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if LO <= combined < HI:
                        kind = "STORE" if mnem.startswith("s") else "load"
                        hits.setdefault(fa, []).append((str(insn.getAddress()), "{} ({})".format(mnem, kind), "0x{:08X}".format(combined)))
        except:
            pass

print("scanned {} instructions".format(total))
print("functions touching 0x{:08X}-0x{:08X}: {}".format(LO, HI, len(hits)))
# sort by number of hits, descending
for fa, refs in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa))) if fa else None
    fname = func.getName() if func else "?"
    # filter store hits
    stores = [r for r in refs if "STORE" in r[1]]
    loads = [r for r in refs if "load" in r[1]]
    addrs = [r for r in refs if "lui+addiu" in r[1]]
    print("\n  {} @ 0x{:08X}  stores={} loads={} addrs={}".format(fname, fa or 0, len(stores), len(loads), len(addrs)))
    for ia, kind, tgt in stores[:8]:
        print("    [W] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in loads[:4]:
        print("    [r] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in addrs[:4]:
        print("    [a] {}  {}  {}".format(ia, kind, tgt))
