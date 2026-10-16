# @category Legaia
# @runtime Jython
#
# For every program, ask the reference manager for ALL references TO each
# TARGET address (data refs, computed-call refs, etc.) and also report whether
# a function/symbol is defined at that address. Output -> /scripts/funcs.

import os

TARGETS = [0x801D71F0, 0x800421D4, 0x80042EE0, 0x80043048]
OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

state = getState()
project = state.getProject()
pdata = project.getProjectData()
root = pdata.getRootFolder()

from ghidra.util.task import ConsoleTaskMonitor
monitor = ConsoleTaskMonitor()

lines = []


def scan(prog, pname):
    af = prog.getAddressFactory()
    fm = prog.getFunctionManager()
    rm = prog.getReferenceManager()
    listing = prog.getListing()
    got = []
    for t in TARGETS:
        a = af.getDefaultAddressSpace().getAddress(t & 0xFFFFFFFF)
        refs = list(rm.getReferencesTo(a))
        if not refs:
            continue
        fnat = fm.getFunctionAt(a)
        got.append((t, fnat.getName() if fnat else "-", refs))
    if got:
        lines.append("program: %s" % pname)
        for t, name, refs in got:
            lines.append("  TARGET 0x%08X (%s): %d ref(s)" % (t, name, len(refs)))
            for r in refs:
                fa = r.getFromAddress()
                fn = fm.getFunctionContaining(fa)
                ins = listing.getInstructionAt(fa)
                lines.append("    from %s [%s] in %s : %s" % (
                    fa, r.getReferenceType(),
                    fn.getName() if fn else "-",
                    ins.toString() if ins else "?"))


def walk(folder):
    for f in folder.getFiles():
        name = f.getName()
        try:
            obj = f.getReadOnlyDomainObject(state, -1, monitor)
        except Exception as e:
            lines.append("program: %s  (FAILED: %s)" % (name, e))
            continue
        try:
            scan(obj, name)
        finally:
            obj.release(state)
    for sub in folder.getFolders():
        walk(sub)


walk(root)
out = "%s/refs_to_giveitem.txt" % OUT_DIR
with open(out, "w") as fh:
    fh.write("\n".join(lines))
print("\n".join(lines))
print("--- report -> %s" % out)
