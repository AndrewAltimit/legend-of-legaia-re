# @category Legaia
# @runtime Jython
#
# Wave-8 Arc 6: locate every SCUS-resident access (load/store/addiu) whose
# LUI+offset effective address is _DAT_8007b85c (the 0x62C00 asset-buffer
# base). The scene-v12 record-table consumer lead is an un-analyzed reader
# near ~0x800219xx. Reports (function entry, insn addr, kind, target) for
# each hit so the containing function(s) can be dumped.
#
# Same walk as find_lui_writers.py, pinned to the single-word window.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

LO = 0x8007B85C
HI = 0x8007B85F

hits = []

inst_iter = listing.getInstructions(True)
last_lui = {}
current_func_addr = None
while inst_iter.hasNext():
    insn = inst_iter.next()
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        last_lui = {}
        current_func_addr = fa
    mnem = insn.getMnemonicString()
    ops = insn.getNumOperands()
    if mnem == "lui" and ops == 2:
        reg = insn.getRegister(0)
        sc = insn.getScalar(1)
        if reg is not None and sc is not None:
            last_lui[reg.getName()] = (insn.getAddress(), sc.getUnsignedValue() << 16)
        continue
    if mnem in ("addiu", "ori") and ops == 3:
        dst = insn.getRegister(0)
        src = insn.getRegister(1)
        sc = insn.getScalar(2)
        if dst is not None and src is not None and sc is not None:
            base = last_lui.get(src.getName())
            if base is not None:
                target = base[1] + sc.getSignedValue()
                if LO <= target <= HI:
                    hits.append((fa, insn.getAddress(), mnem, target))
            if dst.getName() in last_lui and dst != src:
                del last_lui[dst.getName()]
        continue
    # loads/stores: reg-offset(base)
    if mnem in ("lw", "lh", "lhu", "lb", "lbu", "sw", "sh", "sb"):
        for i in range(ops):
            for obj in insn.getOpObjects(i):
                pass
        try:
            sc = insn.getScalar(1)
            base_reg = None
            objs = insn.getOpObjects(1)
            for o in objs:
                nm = o.__class__.__name__
                if nm == "Register":
                    base_reg = o.getName()
                elif nm == "Scalar" and sc is None:
                    sc = o
        except Exception:
            sc = None
            base_reg = None
        if base_reg is not None:
            base = last_lui.get(base_reg)
            if base is not None:
                off = sc.getSignedValue() if sc is not None else 0
                target = base[1] + off
                if LO <= target <= HI:
                    hits.append((fa, insn.getAddress(), mnem, target))
        # a load into the lui-tracked register invalidates it
        d = insn.getRegister(0)
        if d is not None and d.getName() in last_lui and mnem.startswith("l"):
            del last_lui[d.getName()]

print("=== _DAT_8007b85c access sites: %d ===" % len(hits))
for fa, addr, kind, target in hits:
    fs = ("FUN_%08x" % fa) if fa else "(no func)"
    print("%s  insn=%s  %s  -> 0x%08x" % (fs, addr, kind, target))
print("=== end ===")
