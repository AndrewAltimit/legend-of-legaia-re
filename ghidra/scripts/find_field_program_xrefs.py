# @category Legaia
# @runtime Jython
#
# Locate strings related to the script VM and find their xrefs:
#   - "---- FIELD PROGRAM -----%d"  (file 0x8a8 -> RAM 0x800100A8)
#   - "MAP NAME %s"
#   - "map work %d"
#   - "BATTLE MODE", "MAP MODE", "MAP TEST"
#   - "program_no=%d" (already known, used by FUN_80016230)
#   - "..\\..\\FIELD\\PROGRAM\\..." (the dev path for field programs)

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
fm = prog.getFunctionManager()
mem = prog.getMemory()

# File offset -> RAM address (PSX-EXE: file 0x800 -> RAM 0x80010000)
def f2r(off):
    return 0x80010000 + (off - 0x800)

CANDIDATES = [
    (f2r(0x8a8), "FIELD PROGRAM banner"),
    (f2r(0x9e4), "MAP NAME %s"),
    (f2r(0xa04), "map work %d"),
    (f2r(0x11f4), "BATTLE MODE"),
    (f2r(0x1228), "MAP MODE"),
    (f2r(0x1234), "MAP TEST"),
    (f2r(0x1240), "MAPDSIP MODE"),
    (f2r(0x1250), "MAPDSIP MODE INIT"),
    (f2r(0xc64), "initmap.txt"),
    (f2r(0xc90), "DATA\\FIELD\\"),
]

print("Probing candidate strings:")
for ram, label in CANDIDATES:
    a = af.getAddress("{:x}".format(ram))
    bs = []
    for j in range(28):
        try:
            bs.append(mem.getByte(a.add(j)) & 0xFF)
        except:
            bs.append(0)
    ascii_repr = "".join(chr(b) if 0x20 <= b < 0x7F else "." for b in bs)
    print("  0x{:08X}  {!r:24s}  {}".format(ram, label, ascii_repr))

print("\n--- LUI scan: refs to candidate addresses ---")
target_set = {ram for ram, _ in CANDIDATES}
labels = dict(CANDIDATES)
for ram, _ in CANDIDATES:
    labels[ram] = labels[ram]

inst_iter = listing.getInstructions(True)
last_lui = {}
cur_func = None
hits = {a: [] for a in target_set}

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
                base = last_lui[src]
                combined = (base + imm) & 0xFFFFFFFF
                if combined in target_set:
                    fname = func.getName() if func else "?"
                    fe = "0x{:08X}".format(fa) if fa else "?"
                    hits[combined].append((str(insn.getAddress()), fname, fe))
                last_lui[dst] = combined
        except: pass
        continue

for ram in sorted(target_set):
    items = hits[ram]
    print("\n0x{:08X}  ({}):  {} ref(s)".format(ram, labels[ram], len(items)))
    seen_funcs = set()
    for ia, fn, fe in items:
        if fe not in seen_funcs:
            seen_funcs.add(fe)
            print("  {}  {} @ {}".format(ia, fn, fe))
        else:
            print("  {}  (same fn)".format(ia))

print("\ndone")
