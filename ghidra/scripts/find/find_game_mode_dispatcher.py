# @category Legaia
# @runtime Jython
#
# Locate the game-mode dispatcher (state machine) and the script-VM execution
# function for mode 3 (FIELD PROGRAM).
#
# What we know:
#   - FUN_80016230 reads the game-state at gp[0x524] and prints the banner
#     "---- FIELD PROGRAM -----%d" when state == 3.
#   - "MAP NAME %s" is referenced by FUN_80016444.
#   - "DATA\\FIELD\\" is referenced by FUN_8001f7c0 and FUN_80020118.
#
# Goal: find the master switch that calls per-mode handlers.

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
fm = prog.getFunctionManager()

TARGETS = [
    (0x80016230, "FUN_80016230 (FIELD PROGRAM dev-print)"),
    (0x80016444, "FUN_80016444 (MAP NAME dev-print)"),
    (0x8001f7c0, "FUN_8001f7c0 (DATA\\FIELD\\ user 1)"),
    (0x80020118, "FUN_80020118 (DATA\\FIELD\\ user 2)"),
    (0x8001d424, "FUN_8001d424 (initmap.txt user)"),
]

for tgt, label in TARGETS:
    addr = af.getAddress("{:x}".format(tgt))
    refs = list(ref_mgr.getReferencesTo(addr))
    print("\n== refs to {} ({} ref(s)) ==".format(label, len(refs)))
    by_func = {}
    for r in refs:
        from_a = r.getFromAddress()
        rt = r.getReferenceType()
        func = fm.getFunctionContaining(from_a)
        fname = func.getName() if func else "?"
        fentry = str(func.getEntryPoint()) if func else "?"
        by_func.setdefault((fentry, fname), []).append((str(from_a), str(rt)))
    for (fe, fn), items in sorted(by_func.items()):
        print("  {} @ {}  ({} call(s))".format(fn, fe, len(items)))
        for ia, rt in items[:3]:
            print("    {}  {}".format(ia, rt))

# Also: find functions that read gp[0x524] (the game-mode register).
# GP is set by the runtime; for PSX-EXE the start file typically has
# `lw gp, 0x14(<header>)` then GP = some fixed value. We need to find that.
# Pragmatic shortcut: scan for ANY load with `lh ?,0x524(gp)` and report the
# function -- that's a mode-checker.
print("\n=== functions that read gp[0x524] (game-mode register) ===")
inst_iter = listing.getInstructions(True)
fn_hits = {}
for insn in iter(inst_iter.next, None):
    pass

# Re-iterate properly (Jython generators)
inst_iter = listing.getInstructions(True)
total = 0
while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    mnem = insn.getMnemonicString()
    if mnem not in ("lh", "lhu", "lw", "lb", "lbu"):
        continue
    if insn.getNumOperands() != 2:
        continue
    try:
        op2 = insn.getDefaultOperandRepresentation(1)
    except:
        continue
    if "(gp)" not in op2:
        continue
    if "0x524" not in op2 and "0x524" not in op2:
        continue
    func = fm.getFunctionContaining(insn.getAddress())
    if not func:
        continue
    key = (str(func.getEntryPoint()), func.getName(), mnem)
    fn_hits.setdefault(key, []).append(str(insn.getAddress()))

for (fe, fn, mn), addrs in sorted(fn_hits.items()):
    print("  {} @ {}  {}  ({} read(s))".format(fn, fe, mn, len(addrs)))
    for a in addrs[:4]:
        print("    {}".format(a))

print("\nscanned {} instructions".format(total))
print("done")
