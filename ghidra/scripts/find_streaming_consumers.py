# @category Legaia
# @runtime Jython
#
# Trail the DATA_FIELD streaming buffer consumer chain.
#
# 1. Callers of FUN_8002541c (streaming-asset driver). For each call, capture
#    the surrounding instructions (arg prep + post-call code) -- post-call code
#    is what would touch the trailer.
# 2. Direct refs to 0x8007b85c (the global buffer pointer). Anyone besides
#    8002541c / 800255b8 reading it is a candidate trailer consumer.
# 3. Direct refs to 0x80084540 (the asset-table-index global, written by
#    800255b8's else-branch) -- might point at a related lookup.

import os

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

OUT = open(os.path.join(OUT_DIR, "streaming_consumers.txt"), "w")
def w(line=""):
    OUT.write(line + "\n")

# ---------------------------------------------------------------------------
# 1. Callers of FUN_8002541c, with post-call window
# ---------------------------------------------------------------------------
TGT_FUNC = 0x8002541c
addr = af.getAddress("{:x}".format(TGT_FUNC))
refs = list(ref_mgr.getReferencesTo(addr))
w("== Callers of FUN_{:08x} ({} xrefs) ==".format(TGT_FUNC, len(refs)))
callers = {}
for r in refs:
    if not r.getReferenceType().isCall():
        continue
    from_a = r.getFromAddress()
    func = fm.getFunctionContaining(from_a)
    if func is None:
        continue
    fentry = func.getEntryPoint().getOffset()
    callers.setdefault(fentry, []).append(from_a)

for fentry in sorted(callers):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fentry)))
    fname = func.getName() if func else "?"
    w("\n  {} @ 0x{:08X} ({} call sites)".format(fname, fentry, len(callers[fentry])))
    for site in callers[fentry]:
        w("    --- call site at {} ---".format(site))
        # 4 insns before, 12 after (post-call is the trailer-consumer territory)
        cur = site
        # walk back 4
        prev = []
        a = listing.getInstructionBefore(cur)
        while a and len(prev) < 4:
            prev.append(a)
            a = listing.getInstructionBefore(a.getAddress())
        for ins in reversed(prev):
            w("      pre  {}  {}  {}".format(ins.getAddress(), ins.getMnemonicString(), ins))
        site_ins = listing.getInstructionAt(site)
        if site_ins:
            w("      CALL {}  {}  {}".format(site_ins.getAddress(), site_ins.getMnemonicString(), site_ins))
        # delay slot is the next instruction
        nxt = listing.getInstructionAfter(site)
        cnt = 0
        cur = nxt
        while cur and cnt < 16:
            w("      post {}  {}  {}".format(cur.getAddress(), cur.getMnemonicString(), cur))
            cnt += 1
            cur = listing.getInstructionAfter(cur.getAddress())

# ---------------------------------------------------------------------------
# 2. Refs to the buffer global 0x8007b85c
# ---------------------------------------------------------------------------
w("\n\n== Refs to _DAT_8007b85c (buffer ptr) ==")
addr = af.getAddress("8007b85c")
refs = list(ref_mgr.getReferencesTo(addr))
by_func = {}
for r in refs:
    from_a = r.getFromAddress()
    rt = r.getReferenceType()
    func = fm.getFunctionContaining(from_a)
    fname = func.getName() if func else "?"
    fentry_str = str(func.getEntryPoint()) if func else "?"
    kind = "W" if rt.isWrite() else ("R" if rt.isRead() else "?")
    insn = listing.getInstructionAt(from_a)
    mnem = insn.getMnemonicString() if insn else "?"
    by_func.setdefault((fentry_str, fname), []).append((kind, str(from_a), mnem))

for (fentry, fname), hits in sorted(by_func.items()):
    kinds = "".join(h[0] for h in hits)
    w("  {} @ {}  [{}]  ({} refs)".format(fname, fentry, kinds, len(hits)))
    for k, a, m in hits:
        w("    [{}] {} {}".format(k, a, m))

# ---------------------------------------------------------------------------
# 3. Refs to 0x80084540 (asset table index?)
# ---------------------------------------------------------------------------
w("\n\n== Refs to _DAT_80084540 ==")
addr = af.getAddress("80084540")
refs = list(ref_mgr.getReferencesTo(addr))
by_func = {}
for r in refs:
    from_a = r.getFromAddress()
    rt = r.getReferenceType()
    func = fm.getFunctionContaining(from_a)
    fname = func.getName() if func else "?"
    fentry_str = str(func.getEntryPoint()) if func else "?"
    kind = "W" if rt.isWrite() else ("R" if rt.isRead() else "?")
    by_func.setdefault((fentry_str, fname), []).append((kind, str(from_a)))

for (fentry, fname), hits in sorted(by_func.items()):
    kinds = "".join(h[0] for h in hits)
    w("  {} @ {}  [{}]  ({} refs)".format(fname, fentry, kinds, len(hits)))

OUT.close()
print("wrote /scripts/funcs/streaming_consumers.txt")
