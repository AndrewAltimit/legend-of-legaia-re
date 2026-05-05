# @category Legaia
# @runtime Jython
#
# Count COP2 / GTE instructions per function in SCUS_942.54.
#
# Any function that transforms vertices in software OR uses the PSX GTE for
# 3D math will issue COP2 instructions (mfc2, mtc2, ctc2, cfc2, lwc2, swc2,
# RTPS/RTPT, AVSZ3/AVSZ4, NCLIP, NCDS, etc.). The TMD renderer must do
# vertex transforms, so it must use the GTE.
#
# If COP2 use is absent or trivial in SCUS_942.54, that's strong evidence
# the renderer lives in the overlay code at 0x801C0000+.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# COP2 instructions on MIPS R3000A:
# - mfc2, mtc2, cfc2, ctc2 (move between CPU and GTE)
# - lwc2, swc2 (load/store GTE word)
# - All GTE ops are encoded as COP2 with bit 25 set; mnemonics include:
#   rtps rtpt nclip avsz3 avsz4 mvmva ncds ncdt nccs ncct ncs nct gpf gpl dcpl dpcs intpl
COP2_MNEMS = {
    "mfc2", "mtc2", "cfc2", "ctc2", "lwc2", "swc2",
    "rtps", "rtpt", "nclip", "avsz3", "avsz4",
    "mvmva", "ncds", "ncdt", "nccs", "ncct", "ncs", "nct",
    "gpf", "gpl", "dcpl", "dpcs", "intpl", "sqr", "op",
    "dpct", "cdp",
}

per_func = {}
total_cop2 = 0
for insn in listing.getInstructions(True):
    mnem = insn.getMnemonicString().lower()
    if mnem.startswith("_"):
        mnem = mnem[1:]
    if mnem in COP2_MNEMS:
        total_cop2 += 1
        f = fm.getFunctionContaining(insn.getAddress())
        if f is None:
            continue
        fa = f.getEntryPoint().getOffset()
        per_func.setdefault(fa, []).append((str(insn.getAddress()), mnem))

print("Total COP2/GTE instructions across SCUS_942.54: {}".format(total_cop2))
print("Functions with any COP2 use: {}".format(len(per_func)))
ranked = sorted(per_func.items(), key=lambda kv: (-len(kv[1]),))
for fa, hits in ranked[:30]:
    f = fm.getFunctionAt(af.getAddress("{:x}".format(fa)))
    fname = f.getName() if f else "?"
    mnem_counts = {}
    for _, m in hits:
        mnem_counts[m] = mnem_counts.get(m, 0) + 1
    print("  {} @ 0x{:08X}  count={}  mnems={}".format(fname, fa, len(hits), mnem_counts))
