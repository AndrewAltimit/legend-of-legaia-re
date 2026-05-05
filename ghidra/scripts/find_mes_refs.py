# @category Legaia
# @runtime Jython
#
# Multi-strategy search for users of _DAT_8007b8a8 (MES buffer pointer):
#   1. Ghidra's reference manager (catches refs Ghidra has already resolved).
#   2. Disassembly-text scan for the LUI/ADDIU pair that builds 0x8007b8a8.
#      LUI imm = 0x8008, ADDIU/store offset = -0x4758.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

TARGET = 0x8007b8a8

# Strategy 1: reference manager
print("== Strategy 1: ref_mgr.getReferencesTo(0x{:08X}) ==".format(TARGET))
addr = af.getAddress("{:x}".format(TARGET))
refs = list(ref_mgr.getReferencesTo(addr))
print("found {} refs".format(len(refs)))
by_func = {}
for r in refs:
    from_a = r.getFromAddress()
    func = fm.getFunctionContaining(from_a)
    fentry = func.getEntryPoint().getOffset() if func else 0
    fname = func.getName() if func else "(none)"
    by_func.setdefault((fentry, fname), []).append(from_a)
for (fentry, fname), sites in sorted(by_func.items()):
    print("  {} @ 0x{:08X}  ({} refs)".format(fname, fentry, len(sites)))
    for s in sites[:3]:
        ins = listing.getInstructionAt(s)
        print("    {}  {}".format(s, ins.toString() if ins else "?"))

# Strategy 2: scan the whole disassembly for the LUI 0x8008 pattern, then
# look 1-3 instructions ahead for an addiu/store that resolves to TARGET.
print("\n== Strategy 2: LUI 0x8008 + offset -0x4758 scan ==")
EXPECTED_OFFSET = -0x4758  # signed 16-bit (0xB8A8)

count = 0
hits_by_func = {}
it = listing.getInstructions(True)
while it.hasNext():
    ins = it.next()
    mnem = ins.getMnemonicString().lower()
    if mnem != "lui":
        continue
    try:
        imm = ins.getOpObjects(1)[0].getValue() & 0xFFFF
    except Exception:
        continue
    if imm != 0x8008:
        continue
    # Walk up to 4 instructions ahead, looking for any operand referencing
    # the offset -0x4758 or +0xB8A8 (the LUI-paired residual).
    cursor = ins
    for _ in range(4):
        cursor = listing.getInstructionAfter(cursor.getAddress())
        if cursor is None:
            break
        s = cursor.toString()
        if "-0x4758" in s or "0xb8a8" in s.lower():
            func = fm.getFunctionContaining(cursor.getAddress())
            fentry = func.getEntryPoint().getOffset() if func else 0
            fname = func.getName() if func else "(none)"
            hits_by_func.setdefault((fentry, fname), []).append(
                (ins.getAddress(), cursor.getAddress(), cursor.toString())
            )
            count += 1
            break

print("found {} hits in {} function(s)".format(count, len(hits_by_func)))
for (fentry, fname), sites in sorted(hits_by_func.items()):
    print("\n  {} @ 0x{:08X}  ({} sites)".format(fname, fentry, len(sites)))
    for lui_a, use_a, use_text in sites[:5]:
        print("    lui @ {} -> use @ {}: {}".format(lui_a, use_a, use_text))
