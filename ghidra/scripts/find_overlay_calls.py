# @category Legaia
# @runtime Jython
#
# Find every call (jal or jalr-with-resolved-target) into the RAM-resident
# overlay region 0x801C0000 - 0x801FFFFF. These are calls into dynamically
# loaded overlays. We expect the script VM and field-program runtime to live
# there, since the executable itself contains no mode dispatcher.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

LO = 0x801C0000
HI = 0x80200000

inst_iter = listing.getInstructions(True)
hits = {}
total = 0
last_lui = {}
current_func = None

while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    f = fm.getFunctionContaining(insn.getAddress())
    fa = f.getEntryPoint().getOffset() if f else None
    if fa != current_func:
        last_lui = {}
        current_func = fa
    mnem = insn.getMnemonicString()
    if mnem == "jal" and insn.getNumOperands() == 1:
        try:
            tgt = insn.getOpObjects(0)[0].getOffset()
        except:
            continue
        if LO <= tgt < HI:
            f_e = "0x{:x}".format(fa) if fa else "?"
            f_n = f.getName() if f else "?"
            hits.setdefault((f_e, f_n), []).append(("jal", str(insn.getAddress()), "0x{:x}".format(tgt)))
        continue
    if mnem == "lui":
        try:
            reg = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except:
            pass
        continue
    if mnem == "addiu" and insn.getNumOperands() == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue()
        except:
            continue
        if src in last_lui:
            combined = (last_lui[src] + imm) & 0xFFFFFFFF
            if LO <= combined < HI:
                f_e = "0x{:x}".format(fa) if fa else "?"
                f_n = f.getName() if f else "?"
                hits.setdefault((f_e, f_n), []).append(("addr-load", str(insn.getAddress()), "0x{:x}".format(combined)))
            last_lui[dst] = combined
        else:
            last_lui.pop(dst, None)
        continue

print("=== calls / address-loads into RAM overlay region (0x801C0000+) ===")
target_freq = {}
for (fe, fn), items in sorted(hits.items()):
    print("  {} @ {}  ({} hit(s))".format(fn, fe, len(items)))
    for kind, ia, tgt in items[:5]:
        print("    {} {} -> {}".format(ia, kind, tgt))
        target_freq[tgt] = target_freq.get(tgt, 0) + 1

print("\n=== unique overlay targets ranked by call site count ===")
for tgt, c in sorted(target_freq.items(), key=lambda kv: -kv[1])[:20]:
    print("  {} -> {} call site(s)".format(tgt, c))

print("\nscanned {} insns".format(total))
print("done")
