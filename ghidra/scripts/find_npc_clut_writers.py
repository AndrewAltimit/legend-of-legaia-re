# @category Legaia
# @runtime Jython
#
# Find every function in EVERY program (SCUS_942.54 + every imported
# overlay) that reads/writes/computes any address in the NPC-CLUT
# staging buffer at 0x800F19B0 (and the broader CLUT-table window
# around it).
#
# Empirically the runtime stages 7 contiguous 32-byte CLUTs at
# 0x800F19B0..0x800F1A90 (one per row-479 slot 8..14) before DMA'ing
# them to VRAM row 479 x=128..240. The bytes have HSV-hue-cycle shape
# and are not present anywhere on disc, so a generator-or-staging-copy
# function must materialise them.
#
# Run with `-recursive -readOnly` to walk every imported program.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# Slot 8..15 of row 479 = a single 0x100-byte window. We scan a bit
# wider to catch base-register pointers that land slightly outside.
LO = 0x800F1800
HI = 0x800F1B00

hits = {}
inst_iter = listing.getInstructions(True)
total = 0
last_lui = {}
current_func_addr = None

for insn in inst_iter:
    total += 1
    addr = insn.getAddress()
    func = fm.getFunctionContaining(addr)
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        last_lui = {}
        current_func_addr = fa
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
    if mnem in ("sw", "sh", "sb", "lw", "lh", "lhu", "lb", "lbu", "swl", "swr") and ops == 2:
        try:
            op = insn.getDefaultOperandRepresentation(1)
            if "(" in op and op.endswith(")"):
                offstr, basestr = op.split("(")
                basestr = basestr.rstrip(")")
                off = None
                try:
                    if offstr.startswith(("0x", "-0x")):
                        off = int(offstr, 16)
                    else:
                        off = int(offstr, 0)
                except:
                    off = None
                if off is not None and basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if LO <= combined < HI:
                        kind = "STORE" if mnem.startswith("s") else "load"
                        hits.setdefault(fa, []).append((str(addr), "{} ({})".format(mnem, kind), "0x{:08X}".format(combined)))
        except:
            pass

print("[{}] scanned {} instructions; hit functions={}".format(prog.getName(), total, len(hits)))
for fa, refs in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa))) if fa else None
    fname = func.getName() if func else "?"
    stores = [r for r in refs if "STORE" in r[1]]
    loads = [r for r in refs if "load" in r[1]]
    addrs = [r for r in refs if r[1] == "addr"]
    print("  [{}] {} @ 0x{:08X}  stores={} loads={} addrs={}".format(prog.getName(), fname, fa or 0, len(stores), len(loads), len(addrs)))
    for ia, kind, tgt in stores[:6]:
        print("    [W] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in loads[:4]:
        print("    [r] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in addrs[:4]:
        print("    [a] {}  {}  {}".format(ia, kind, tgt))
