# @category Legaia
# @runtime Jython
#
# Find every instruction that WRITES to a small window of $gp-relative slots
# around the drawable-list head at gp[0x148]. PSX games using PsyQ's libgs
# typically anchor a 4-slot block here:
#
#   gp[0x140]   ?
#   gp[0x144]   ?
#   gp[0x148]   drawable-list HEAD pointer (read by FUN_80031D00 walker;
#               consumed by FUN_8002C69C continent-terrain emitter)
#   gp[0x14C]   mode_byte (latched per node before each per-node emit call)
#   gp[0x150]   ?
#   gp[0x154]   ?
#   gp[0x158]   ?
#   gp[0x15C]   re-entrance / lock flag (FUN_80031D00 toggles around the walk)
#
# We need to find every site that stores into 0x140..0x15C(gp) to identify
# what installs the continent drawable. Anything reachable from the world-map
# controller FUN_801E76D4 (lives in the world_map overlay) is a candidate.
#
# Pattern matched: `sw/sh/sb $r, off($gp)` with off in WINDOW. Ghidra renders
# the gp register as "gp" in the operand string for MIPS.

import os

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

PROGRAM_NAME = prog.getName().replace("/", "_").replace(":", "_")

WINDOW_LO = 0x140
WINDOW_HI = 0x160  # exclusive
INTERESTED = set([0x148, 0x14C, 0x158, 0x15C])

out_dir = "/scripts/funcs"
try:
    os.makedirs(out_dir)
except OSError:
    pass

# Per-function hits.
# func_entry -> list of (insn_addr, mnem, dst_reg, offset, full_repr)
hits = {}

inst_iter = listing.getInstructions(True)
total = 0


def parse_off_base(rep):
    """Parse an operand like '0x148(gp)' -> (0x148, 'gp')."""
    if "(" not in rep or not rep.endswith(")"):
        return None
    a, b = rep.split("(", 1)
    a = a.strip()
    b = b[:-1].strip()
    try:
        if a.startswith("-0x"):
            off = -int(a[3:], 16)
        elif a.startswith("0x"):
            off = int(a[2:], 16)
        else:
            off = int(a)
    except ValueError:
        return None
    return off, b


STORE_MNEMS = ("sw", "sh", "sb")

while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    mnem = insn.getMnemonicString()
    if mnem not in STORE_MNEMS:
        continue
    try:
        # MIPS store: `sw $r, off(base)` -> op0 = $r, op1 = off(base).
        if insn.getNumOperands() < 2:
            continue
        dst_rep = insn.getDefaultOperandRepresentation(0)
        mem_rep = insn.getDefaultOperandRepresentation(1)
    except Exception:
        continue
    parsed = parse_off_base(mem_rep)
    if parsed is None:
        continue
    off, base = parsed
    if base != "gp":
        continue
    if not (WINDOW_LO <= off < WINDOW_HI):
        continue
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    hits.setdefault(fa, []).append(
        (str(insn.getAddress()), mnem, dst_rep, off, insn.toString())
    )

# Report.
lines = []
lines.append("program: %s" % prog.getName())
lines.append("instructions scanned: %d" % total)
lines.append(
    "window: 0x%X..0x%X (gp); interested: %s"
    % (WINDOW_LO, WINDOW_HI, ", ".join("0x%X" % v for v in sorted(INTERESTED)))
)
lines.append("functions writing to window: %d\n" % len(hits))

# Rank: prioritize functions that hit any INTERESTED slot.
def func_key(item):
    fa, refs = item
    interesting = any(r[3] in INTERESTED for r in refs)
    return (0 if interesting else 1, -len(refs))


for fa, refs in sorted(hits.items(), key=func_key):
    func = fm.getFunctionAt(af.getAddress("%x" % fa)) if fa is not None else None
    fname = func.getName() if func else "?"
    interesting = sorted(set(r[3] for r in refs if r[3] in INTERESTED))
    star = "*" if interesting else " "
    lines.append(
        "%s FUN_%08X %-32s hits=%d interesting=[%s]"
        % (
            star,
            fa or 0,
            fname,
            len(refs),
            ", ".join("0x%X" % v for v in interesting),
        )
    )
    for ia, mnem, dst, off, full in refs[:12]:
        lines.append("    %s  %s  %s, 0x%X(gp)   %s" % (ia, mnem, dst, off, full))
    if len(refs) > 12:
        lines.append("    ... %d more" % (len(refs) - 12))
    lines.append("")

out_path = "%s/gp_drawable_writers_%s.txt" % (out_dir, PROGRAM_NAME)
with open(out_path, "w") as fh:
    fh.write("\n".join(lines))

# Echo a short summary to console.
print("\n".join(lines[: min(80, len(lines))]))
print("---")
print("full report -> %s" % out_path)
