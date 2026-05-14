# @category Legaia
# @runtime Jython
#
# Probe Ghidra's reference manager for writers/readers to a configurable
# set of world-map subsystem globals. Useful for tracing the installer
# that populates DAT_8007C018 (the per-kingdom-per-kind data pointer
# table) and its companion globals.
#
# Target list (current):
#   - 0x8007BB38: table count global (DAT_8007BB38 + 1 entries in table)
#   - 0x8007B7DC: second-table base pointer (used by FUN_801D77F4)
#   - 0x8007BA74: vertex-count accumulator (set by FUN_801D77F4)
#   - 0x8007068C: actor pool base (allocator FUN_80020DE0 arg)
#   - 0x800701A8: alt pool base from FUN_80020DE0 callers (TBD)
#
# Run against any program (SCUS, overlays). Aggregate findings across runs.

from ghidra.program.model.symbol import RefType

prog = currentProgram
prog_name = prog.getName()
ref_mgr = prog.getReferenceManager()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
mem = prog.getMemory()

TARGETS = [
    (0x8007BB38, "DAT_8007BB38_count"),
    (0x8007B7DC, "DAT_8007B7DC_second_table"),
    (0x8007BA74, "DAT_8007BA74_vtx_accum"),
    (0x8007068C, "DAT_8007068C_actor_pool"),
    (0x80070628, "near_pool_a"),
    (0x80070630, "near_pool_b"),
]

print("=== %s ===" % prog_name)

for addr_int, name in TARGETS:
    target = af.getAddress("%x" % addr_int)
    refs = list(ref_mgr.getReferencesTo(target))
    if not refs:
        continue
    writers = []
    readers = []
    other = []
    for ref in refs:
        rt = ref.getReferenceType()
        from_addr = ref.getFromAddress()
        func = fm.getFunctionContaining(from_addr)
        fname = func.getName() if func else "?"
        entry = "%s @ %s -> 0x%08X (%s)" % (fname, from_addr, addr_int, rt)
        if rt.isWrite():
            writers.append(entry)
        elif rt.isRead():
            readers.append(entry)
        else:
            other.append(entry)
    print("\n*** %s (0x%08X) ***" % (name, addr_int))
    print("  WRITES (%d):" % len(writers))
    for w in writers:
        print("    " + w)
    print("  READS (%d):" % len(readers))
    for r in readers[:8]:
        print("    " + r)
    print("  OTHER (%d):" % len(other))
    for o in other[:4]:
        print("    " + o)
