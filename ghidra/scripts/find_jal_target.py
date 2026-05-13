# @category Legaia
# @runtime Jython
#
# Find every `jal` instruction whose target == TARGET in the currently-loaded
# program. MIPS JAL encodes the target address directly in the 26-bit
# immediate (shifted left 2 + masked into the high 4 bits of PC), so a JAL
# is a single-instruction call. Ghidra's reference manager DOES register
# these as refs, but we also want a quick standalone sweep that prints
# the call site in context (containing function + instruction line).
#
# Edit TARGET below.

import os

TARGET = 0x800326AC
CONTEXT_BEFORE = 6  # instructions of context to print before each hit
CONTEXT_AFTER = 1   # delay-slot + 1 follow-up

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

PROGRAM_NAME = prog.getName().replace("/", "_").replace(":", "_")

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

hits = []  # (insn_addr_str, func_name, func_entry)
inst_iter = listing.getInstructions(True)
total = 0
while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    mnem = insn.getMnemonicString()
    if mnem != "jal":
        continue
    try:
        # jal <target>
        target_obj = insn.getOpObjects(0)[0]
        target = target_obj.getOffset() if hasattr(target_obj, "getOffset") else int(str(target_obj), 0)
    except Exception:
        continue
    if (target & 0xFFFFFFFF) != (TARGET & 0xFFFFFFFF):
        continue
    func = fm.getFunctionContaining(insn.getAddress())
    fname = func.getName() if func else "?"
    fa = func.getEntryPoint().getOffset() if func else 0
    hits.append((str(insn.getAddress()), fname, fa, insn))

lines = []
lines.append("program: %s" % prog.getName())
lines.append("instructions scanned: %d" % total)
lines.append("jal targeting 0x%08X: %d hit(s)\n" % (TARGET, len(hits)))

for ia, fname, fa, insn in hits:
    lines.append("=== %s in %s @ 0x%08X ===" % (ia, fname, fa))
    # Print a small window of preceding instructions to show how the call
    # site sets up its arguments (a0, a1).
    cur = insn.getPrevious()
    pre = []
    for _ in range(CONTEXT_BEFORE):
        if cur is None:
            break
        pre.append(cur)
        cur = cur.getPrevious()
    for prev in reversed(pre):
        lines.append("    %s  %s" % (prev.getAddress(), prev.toString()))
    lines.append("--> %s  %s" % (insn.getAddress(), insn.toString()))
    nxt = insn.getNext()
    for _ in range(CONTEXT_AFTER):
        if nxt is None:
            break
        lines.append("    %s  %s" % (nxt.getAddress(), nxt.toString()))
        nxt = nxt.getNext()
    lines.append("")

out_path = "%s/jal_xref_%08x_%s.txt" % (OUT_DIR, TARGET, PROGRAM_NAME)
with open(out_path, "w") as f:
    f.write("\n".join(lines))

print("\n".join(lines))
print("---")
print("full report -> %s" % out_path)
