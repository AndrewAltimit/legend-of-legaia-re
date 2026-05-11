# @category Legaia
# @runtime Jython
#
# Find functions that build a `(y << 16) | x` PSX GPU DMA destination
# for row y=479 (0x1DF) - the row that holds the runtime-generated NPC
# CLUT slots 8..14 in town/field scenes.
#
# Two shapes:
#   1) Combined word constant: lui/addiu/ori building 0x01DF_xxxx.
#   2) ori/addiu with immediate 0x01DF used as a high half packed into
#      a register before a sw to GP0.
#
# We catch (1) by tracking lui+addiu+ori chains that produce a value
# whose high halfword is 0x01DF, and (2) by tracking any single-instr
# constant that loads 0x01DF.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# We're interested in any function that materialises 0x01DFxxxx (the
# `(479 << 16) | x` family) or that uses 0x01DF as an immediate.

hits = {}

inst_iter = listing.getInstructions(True)
total = 0
last_lui = {}
last_const = {}  # reg -> value
current_func_addr = None

for insn in inst_iter:
    total += 1
    addr = insn.getAddress()
    func = fm.getFunctionContaining(addr)
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        last_lui = {}
        last_const = {}
        current_func_addr = fa
    mnem = insn.getMnemonicString()
    ops = insn.getNumOperands()
    if mnem == "lui" and ops == 2:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            v = (imm << 16) & 0xFFFFFFFF
            last_lui[dst] = v
            last_const[dst] = v
            if (v >> 16) & 0xFFFF == 0x01DF:
                hits.setdefault(fa, []).append((str(addr), "lui_only_y479", "0x{:08X}".format(v)))
        except:
            pass
        continue
    if mnem == "addiu" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue()
            base = last_const.get(src, 0) if src != 'zero' and src != '0' else 0
            combined = (base + imm) & 0xFFFFFFFF
            last_const[dst] = combined
            if (combined >> 16) & 0xFFFF == 0x01DF:
                hits.setdefault(fa, []).append((str(addr), "addiu_y479", "0x{:08X}".format(combined)))
        except:
            pass
        continue
    if mnem == "ori" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue() & 0xFFFF
            base = last_const.get(src, 0)
            combined = (base | imm) & 0xFFFFFFFF
            last_const[dst] = combined
            if (combined >> 16) & 0xFFFF == 0x01DF:
                hits.setdefault(fa, []).append((str(addr), "ori_y479", "0x{:08X}".format(combined)))
        except:
            pass
        continue

print("[{}] scanned {} instructions; hit functions={}".format(prog.getName(), total, len(hits)))
for fa, refs in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa))) if fa else None
    fname = func.getName() if func else "?"
    print("  [{}] {} @ 0x{:08X}  hits={}".format(prog.getName(), fname, fa or 0, len(refs)))
    for ia, kind, tgt in refs[:6]:
        print("    {}  {}  {}".format(ia, kind, tgt))
