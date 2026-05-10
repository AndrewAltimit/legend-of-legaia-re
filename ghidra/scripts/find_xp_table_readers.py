# @category Legaia
# @runtime Jython
#
# Find every function that reads from the retail XP table at 0x8007123C
# (98 u16 cumulative-XP-increment values). Reports the function entry,
# the read instruction address, and the effective address.
#
# This is a one-shot tracer for the post-battle XP-distribution code.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

LO = 0x8007123C
HI = 0x80071300  # 98 u16 entries = 196 bytes, plus a generous slack

hits = {}
inst_iter = listing.getInstructions(True)
total = 0
last_lui = {}
current_func_addr = None

def reset_state(new_func):
    global last_lui, current_func_addr
    last_lui = {}
    current_func_addr = new_func

for insn in inst_iter:
    total += 1
    addr = insn.getAddress()
    func = fm.getFunctionContaining(addr)
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        reset_state(fa)
    mnem = insn.getMnemonicString()
    ops = insn.getNumOperands()
    if mnem == "lui" and ops == 2:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue()
            last_lui[dst] = (imm << 16) & 0xFFFFFFFF
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
                    hits.setdefault(fa, []).append((str(addr), "addr", "0x{:08X}".format(combined)))
                last_lui[dst] = combined
        except:
            pass
        continue
    if mnem in ("lw", "lh", "lhu", "lb", "lbu") and ops == 2:
        try:
            base_reg = insn.getDefaultOperandRepresentation(1)
            if "(" in base_reg and base_reg.endswith(")"):
                offstr, basestr = base_reg.split("(")
                basestr = basestr.rstrip(")")
                off = None
                try:
                    if offstr.startswith("0x") or offstr.startswith("-0x"):
                        off = int(offstr, 16)
                    else:
                        off = int(offstr, 0)
                except:
                    off = None
                if off is not None and basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if LO <= combined < HI:
                        hits.setdefault(fa, []).append((str(addr), mnem, "0x{:08X}".format(combined)))
        except:
            pass

print("scanned {} instructions".format(total))
print("functions touching 0x{:08X}-0x{:08X}: {}".format(LO, HI, len(hits)))
for fa, refs in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa))) if fa else None
    fname = func.getName() if func else "?"
    print("\n  {} @ 0x{:08X}  reads={}".format(fname, fa or 0, len(refs)))
    for ia, kind, tgt in refs:
        print("    {}  {}  {}".format(ia, kind, tgt))
