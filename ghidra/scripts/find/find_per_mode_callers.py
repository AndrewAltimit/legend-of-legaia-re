# @category Legaia
# @runtime Jython
#
# Find any caller (direct or indirect via address-of constant) of any handler
# in the mode table at 0x8007078C. Strategy: walk every instruction; for each
# `jal` (direct call) or computed-address-load whose value matches a known
# handler entry, report the caller.

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
fm = prog.getFunctionManager()
mem = prog.getMemory()

TABLE_BASE = 0x8007078C
ENTRY_SIZE = 0x18
N = 28

handlers = set()
handler_names = {}
for i in range(N):
    a = af.getAddress("{:x}".format(TABLE_BASE + i * ENTRY_SIZE))
    bs = bytearray(ENTRY_SIZE)
    for j in range(ENTRY_SIZE):
        bs[j] = mem.getByte(a.add(j)) & 0xFF
    h = bs[0x10] | (bs[0x11] << 8) | (bs[0x12] << 16) | (bs[0x13] << 24)
    handlers.add(h)
    f = fm.getFunctionAt(af.getAddress("{:x}".format(h)))
    handler_names[h] = f.getName() if f else "0x{:x}".format(h)

print("Looking for callers of {} unique handlers".format(len(handlers)))

# Walk all instructions
inst_iter = listing.getInstructions(True)
hits = {}  # caller_func -> [(insn_addr, kind, target)]

# Track recent lui for combined LUI+ADDIU
last_lui = {}
current_func = None
count = 0
while inst_iter.hasNext():
    insn = inst_iter.next()
    count += 1
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
        if tgt in handlers:
            f_e = "0x{:x}".format(fa) if fa else "?"
            f_n = f.getName() if f else "?"
            hits.setdefault((f_e, f_n), []).append({
                "at": str(insn.getAddress()),
                "kind": "jal",
                "target": "0x{:x} ({})".format(tgt, handler_names[tgt]),
            })
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
            if combined in handlers:
                f_e = "0x{:x}".format(fa) if fa else "?"
                f_n = f.getName() if f else "?"
                hits.setdefault((f_e, f_n), []).append({
                    "at": str(insn.getAddress()),
                    "kind": "lui+addiu (loads handler addr)",
                    "target": "0x{:x} ({})".format(combined, handler_names[combined]),
                })
            last_lui[dst] = combined
        else:
            last_lui.pop(dst, None)
        continue

print("\n=== callers of any per-mode handler ===")
for (fe, fn), items in sorted(hits.items()):
    print("  {} @ {}  ({} ref(s))".format(fn, fe, len(items)))
    for h in items[:8]:
        print("    {} {} -> {}".format(h["at"], h["kind"], h["target"]))

print("\nscanned {} insns".format(count))
print("done")
