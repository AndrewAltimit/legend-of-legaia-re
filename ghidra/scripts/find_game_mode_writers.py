# @category Legaia
# @runtime Jython
#
# Find writers of the game-mode register at gp[0x524] and gp[0x494]
# (gp[0x494] is also referenced near the FIELD PROGRAM logic - check if it's
# a sub-mode or current-actor index).
#
# Also: log unique gp-relative offsets used as game-state-like reads/writes.

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
fm = prog.getFunctionManager()

TARGET_OFFSETS = ["0x524", "0x494", "-0x4654", "-0x4658", "-0x463c", "-0x4624"]

inst_iter = listing.getInstructions(True)
total = 0
fn_writes = {}
fn_reads = {}
gp_offsets_seen = {}
while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    mnem = insn.getMnemonicString()
    if mnem not in ("sh", "sw", "sb", "lh", "lhu", "lw", "lb", "lbu"):
        continue
    if insn.getNumOperands() != 2:
        continue
    try:
        op2 = insn.getDefaultOperandRepresentation(1)
    except:
        continue
    if "(gp)" not in op2:
        continue
    # parse offset
    off_str = op2.split("(")[0]
    if off_str not in TARGET_OFFSETS:
        # also count
        gp_offsets_seen.setdefault(off_str, 0)
        gp_offsets_seen[off_str] += 1
        continue
    func = fm.getFunctionContaining(insn.getAddress())
    if not func:
        continue
    key = (str(func.getEntryPoint()), func.getName(), off_str, mnem)
    target = fn_writes if mnem.startswith("s") else fn_reads
    target.setdefault(key, []).append(str(insn.getAddress()))

print("=== Writers of gp[0x524] / gp[0x494] / gp[-0x4654] / etc ===")
for (fe, fn, off, mn), addrs in sorted(fn_writes.items()):
    print("  WRITE  {} @ {}  off={}  mnem={}  ({} writes)".format(fn, fe, off, mn, len(addrs)))
    for a in addrs[:6]:
        print("    {}".format(a))

print("\n=== Readers of same offsets ===")
for (fe, fn, off, mn), addrs in sorted(fn_reads.items()):
    print("  READ  {} @ {}  off={}  mnem={}  ({} reads)".format(fn, fe, off, mn, len(addrs)))

print("\n=== Top 20 gp-relative offsets by access count (info only) ===")
for off, count in sorted(gp_offsets_seen.items(), key=lambda x: -x[1])[:20]:
    print("  {}  {} accesses".format(off, count))

print("\nscanned {} instructions".format(total))
print("done")
