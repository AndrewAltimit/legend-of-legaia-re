# @category Legaia
# @runtime Jython
#
# Find every occurrence of a given 32-bit address as a data word OR as the
# combined target of a `lui+addiu`/`lui+ori` pair. Covers both:
#   - Function-pointer arrays (e.g. dispatch tables) where the address is
#     stored as raw .data bytes.
#   - Code that materialises the address into a register via the standard
#     MIPS LUI+ADDIU pair (Ghidra's reference manager misses these unless
#     auto-analysis ran with the AddressTable analyser enabled).
#
# Edit TARGET below. Run with -readOnly + -process <program> to query SCUS
# or a captured overlay.

import os

TARGET = 0x800326AC

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
mem = prog.getMemory()

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

PROGRAM_NAME = prog.getName().replace("/", "_").replace(":", "_")

print("=== searching for 0x%08X in %s ===" % (TARGET, PROGRAM_NAME))

# 1. lui+addiu/lui+ori sites that materialise TARGET.
inst_iter = listing.getInstructions(True)
last_lui = {}
current_func = None
code_hits = []
while inst_iter.hasNext():
    insn = inst_iter.next()
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func:
        last_lui = {}
        current_func = fa
    mnem = insn.getMnemonicString()
    if mnem == "lui":
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[dst] = imm << 16
        except Exception:
            pass
        continue
    if mnem in ("addiu", "ori") and insn.getNumOperands() == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue()
            if src in last_lui:
                base = last_lui[src]
                if mnem == "addiu" and imm < 0:
                    combined = (base + imm) & 0xFFFFFFFF
                else:
                    combined = (base + (imm & 0xFFFFFFFF)) & 0xFFFFFFFF
                if combined == TARGET:
                    fname = func.getName() if func else "?"
                    code_hits.append((str(insn.getAddress()), fname, fa))
                last_lui[dst] = combined
        except Exception:
            pass

# 2. .data words that equal TARGET. Walk the entire defined-memory range.
data_hits = []
blocks = list(mem.getBlocks())
target_le = bytes(bytearray([TARGET & 0xFF, (TARGET >> 8) & 0xFF, (TARGET >> 16) & 0xFF, (TARGET >> 24) & 0xFF]))
for blk in blocks:
    if blk.isExecute():
        # Skip code blocks - the lui+addiu pass already covers them.
        continue
    if not blk.isInitialized():
        continue
    start = blk.getStart().getOffset()
    end = blk.getEnd().getOffset() + 1
    # Read in 4 KB chunks to keep memory pressure manageable.
    addr = blk.getStart()
    pos = 0
    chunk_size = 4096
    while pos < (end - start):
        n = min(chunk_size, end - start - pos)
        buf = bytearray(n)
        for i in range(n):
            try:
                buf[i] = mem.getByte(addr.add(pos + i)) & 0xFF
            except Exception:
                buf[i] = 0
        # Slide a 4-byte window looking for target_le; only 4-aligned matches.
        for off in range(0, n - 3, 4):
            if (
                buf[off] == target_le[0]
                and buf[off + 1] == target_le[1]
                and buf[off + 2] == target_le[2]
                and buf[off + 3] == target_le[3]
            ):
                a = addr.add(pos + off)
                data_hits.append((str(a), blk.getName()))
        pos += n

# Report.
out_path = "%s/addr_xref_%08x_%s.txt" % (OUT_DIR, TARGET, PROGRAM_NAME)
lines = []
lines.append("program: %s" % PROGRAM_NAME)
lines.append("target: 0x%08X" % TARGET)
lines.append("")
lines.append("=== lui+addiu / lui+ori sites loading the target (%d) ===" % len(code_hits))
for ia, fn, fa in code_hits:
    lines.append("  %s in %s @ 0x%08X" % (ia, fn, fa or 0))
lines.append("")
lines.append("=== .data word matches (%d) ===" % len(data_hits))
for a, name in data_hits[:200]:
    lines.append("  %s  block=%s" % (a, name))
if len(data_hits) > 200:
    lines.append("  ... %d more" % (len(data_hits) - 200))

with open(out_path, "w") as f:
    f.write("\n".join(lines))

print("\n".join(lines))
print("---")
print("full report -> %s" % out_path)
