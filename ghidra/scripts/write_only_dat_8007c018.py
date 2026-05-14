# @category Legaia
# @runtime Jython
#
# Tight writes-only filter for the DAT_8007C018 / DAT_8007BB38 globals.
# Print only WRITE references (no reads, no data-only refs), so the
# output stays manageable when run across every overlay in the project.

from ghidra.program.model.symbol import RefType

prog = currentProgram
prog_name = prog.getName()
ref_mgr = prog.getReferenceManager()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()

TARGETS = [
    (0x8007C018, "DAT_8007C018_entry0"),
    (0x8007C01C, "DAT_8007C018_entry1"),
    (0x8007C020, "DAT_8007C018_entry2"),
    (0x8007BB38, "DAT_8007BB38_count"),
    (0x8007BA74, "DAT_8007BA74_vtx_accum"),
]

writes_total = 0
out_lines = []
for addr_int, name in TARGETS:
    target = af.getAddress("%x" % addr_int)
    for ref in ref_mgr.getReferencesTo(target):
        rt = ref.getReferenceType()
        if not rt.isWrite():
            continue
        from_addr = ref.getFromAddress()
        func = fm.getFunctionContaining(from_addr)
        fname = func.getName() if func else "?"
        out_lines.append("  %s WRITE -> 0x%08X (%s) by %s @ %s (%s)" % (
            name, addr_int, prog_name, fname, from_addr, rt))
        writes_total += 1

if writes_total:
    print("=== %s : %d writes ===" % (prog_name, writes_total))
    for line in out_lines:
        print(line)
