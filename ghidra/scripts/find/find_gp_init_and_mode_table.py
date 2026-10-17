# @category Legaia
# @runtime Jython
#
# Two probes:
# 1. Find where $gp is initialized (lui gp + addiu gp pattern). PSX SDK code
#    typically sets gp once at startup; finding it lets us resolve all
#    gp-relative offsets to absolute RAM addresses.
# 2. Find all readers of the mode-transition table at 0x80070790
#    (FUN_800179c0 reads this table; entries are 24 bytes; offset +0xa is
#    the "next mode" field). The function pointer for each mode handler
#    is likely also in this table.

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
fm = prog.getFunctionManager()
mem = prog.getMemory()

# --------- 1. Find gp init ---------
print("=== gp initialization ===")
inst_iter = listing.getInstructions(True)
last_lui = {}
cur_func = None
gp_inits = []
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
            if dst == "gp" and src == "gp":
                # add to current gp value
                if "gp" in last_lui:
                    last_lui["gp"] = (last_lui["gp"] + imm) & 0xFFFFFFFF
                continue
            if dst == "gp" and src in last_lui:
                base = last_lui[src]
                combined = (base + imm) & 0xFFFFFFFF
                gp_inits.append((str(insn.getAddress()), fm.getFunctionContaining(insn.getAddress()).getName() if fm.getFunctionContaining(insn.getAddress()) else "?", "0x{:08X}".format(combined)))
                last_lui["gp"] = combined
        except: pass

for ia, fn, val in gp_inits[:20]:
    print("  {}  {}  gp = {}".format(ia, fn, val))

# --------- 2. Find readers of the mode-transition table 0x8007078C ---------
print("\n=== Readers of the mode-transition table (0x80070790 +/- nearby) ===")
# Scan for LUI(0x8007) + addiu(0x78c) combos
inst_iter = listing.getInstructions(True)
last_lui = {}
cur_func = None
hits = []
TARGETS = set(range(0x80070700, 0x80070900))  # ~512 bytes around the table base

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
                combined = (last_lui[src] + imm) & 0xFFFFFFFF
                last_lui[dst] = combined
                if combined in TARGETS:
                    fname = func.getName() if func else "?"
                    fe = "0x{:08X}".format(fa) if fa else "?"
                    hits.append((str(insn.getAddress()), fname, fe, "0x{:08X}".format(combined)))
        except: pass
        continue
    if mnem in ("lh", "lhu", "lw", "lb", "lbu") and ops == 2:
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
                    try: off = int(offstr)
                    except: off = None
                if off is not None and basestr in last_lui:
                    combined = (last_lui[basestr] + off) & 0xFFFFFFFF
                    if combined in TARGETS:
                        fname = func.getName() if func else "?"
                        fe = "0x{:08X}".format(fa) if fa else "?"
                        hits.append((str(insn.getAddress()), fname, fe, "0x{:08X}".format(combined)))
        except: pass

print("Hits (each line = an instruction touching the mode-transition-table region):")
seen_funcs = {}
for ia, fn, fe, addr in hits:
    seen_funcs.setdefault((fe, fn), []).append((ia, addr))
for (fe, fn), items in sorted(seen_funcs.items()):
    print("  {} @ {}  ({} hits)".format(fn, fe, len(items)))
    for ia, a in items[:5]:
        print("    {}  -> {}".format(ia, a))

# --------- 3. Read the actual table bytes ---------
print("\n=== Mode-transition table at 0x80070790 (24 bytes per entry, first 16 entries) ===")
base = af.getAddress("80070790")
for i in range(16):
    a = base.add(i * 0x18)
    bs = []
    for j in range(0x18):
        try:
            bs.append("{:02x}".format(mem.getByte(a.add(j)) & 0xFF))
        except:
            bs.append("??")
    # decode: u16 next_mode at +0xa, plus interpret first u32 / second u32 as possible function pointers
    try:
        u32_0 = mem.getInt(a) & 0xFFFFFFFF
        u32_4 = mem.getInt(a.add(4)) & 0xFFFFFFFF
    except:
        u32_0 = u32_4 = 0
    next_mode_lo = int(bs[0xa], 16)
    next_mode_hi = int(bs[0xb], 16)
    next_mode = (next_mode_hi << 8) | next_mode_lo
    if next_mode > 0x7FFF:
        next_mode -= 0x10000
    print("  mode[{:2d}] @ {}: {}".format(i, a, " ".join(bs)))
    print("           u32[0]=0x{:08X}  u32[1]=0x{:08X}  next_mode={}".format(u32_0, u32_4, next_mode))

print("\ndone")
