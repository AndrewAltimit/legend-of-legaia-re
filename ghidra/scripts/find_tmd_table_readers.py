# @category Legaia
# @runtime Jython
#
# Find functions that read from the TMD pointer table at 0x80080018
# (where FUN_80026b4c stores TMD pointers indexed by DAT_8007b774).
# Uses LUI+ADDIU/LW tracking like find_lui_writers.py.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

LO = 0x80080000
HI = 0x80080100  # narrow window around 0x80080018

hits = {}
inst_iter = listing.getInstructions(True)
last_lui = {}
current_func_addr = None
total = 0
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
                    hits.setdefault(fa, []).append((str(insn.getAddress()), "addr", "0x{:08X}".format(combined)))
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
                off = int(offstr, 16) if offstr.startswith("0x") else int(offstr, 0) if offstr.startswith("-0x") or offstr[0] in "-+0123456789" else None
                if off is not None and basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if LO <= combined < HI:
                        kind = "STORE" if mnem.startswith("s") else "LOAD"
                        hits.setdefault(fa, []).append((str(insn.getAddress()), "{} ({})".format(mnem, kind), "0x{:08X}".format(combined)))
        except:
            pass

print("scanned {} instructions, found {} touching {:08X}-{:08X}".format(total, len(hits), LO, HI))
# Sort by load count descending - load-heavy functions are the renderers.
def kind_count(refs, kind_str):
    return sum(1 for r in refs if kind_str in r[1])

ranked = sorted(hits.items(), key=lambda kv: (-kind_count(kv[1], "LOAD"), -len(kv[1])))
for fa, refs in ranked[:15]:
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa))) if fa else None
    fname = func.getName() if func else "?"
    nstores = kind_count(refs, "STORE")
    nloads = kind_count(refs, "LOAD")
    print("\n  {} @ 0x{:08X}  loads={} stores={} addrs={}".format(fname, fa or 0, nloads, nstores, len(refs) - nloads - nstores))
    for ia, kind, tgt in refs[:6]:
        print("    {}  {}  {}".format(ia, kind, tgt))
