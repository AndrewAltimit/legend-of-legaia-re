# @category Legaia
# @runtime Jython
#
# Sweep EVERY program in the project for a `jal` (and jump/branch) to TARGET.
# Runs from a single headless invocation by walking the project's domain
# files and opening each program read-only. Output -> /scripts/funcs.

import os

TARGET = 0x801D71F0
CONTEXT_BEFORE = 6
CONTEXT_AFTER = 1

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

lines_all = []


def scan_program(prog, pname):
    listing = prog.getListing()
    fm = prog.getFunctionManager()
    inst_iter = listing.getInstructions(True)
    hits = []
    total = 0
    while inst_iter.hasNext():
        insn = inst_iter.next()
        total += 1
        mnem = insn.getMnemonicString()
        if mnem not in ("jal", "j", "jalr", "bal"):
            continue
        try:
            target_obj = insn.getOpObjects(0)[0]
            target = target_obj.getOffset() if hasattr(target_obj, "getOffset") else None
        except Exception:
            target = None
        if target is None:
            continue
        if (target & 0xFFFFFFFF) != (TARGET & 0xFFFFFFFF):
            continue
        func = fm.getFunctionContaining(insn.getAddress())
        fname = func.getName() if func else "?"
        fa = func.getEntryPoint().getOffset() if func else 0
        hits.append((str(insn.getAddress()), fname, fa, insn, mnem))
    lines_all.append("program: %s  (scanned %d insns, %d hit(s))" % (pname, total, len(hits)))
    for ia, fname, fa, insn, mnem in hits:
        lines_all.append("  === %s [%s] in %s @ 0x%08X ===" % (ia, mnem, fname, fa))
        cur = insn.getPrevious()
        pre = []
        for _ in range(CONTEXT_BEFORE):
            if cur is None:
                break
            pre.append(cur)
            cur = cur.getPrevious()
        for prev in reversed(pre):
            lines_all.append("      %s  %s" % (prev.getAddress(), prev.toString()))
        lines_all.append("  --> %s  %s" % (insn.getAddress(), insn.toString()))
        nxt = insn.getNext()
        for _ in range(CONTEXT_AFTER):
            if nxt is None:
                break
            lines_all.append("      %s  %s" % (nxt.getAddress(), nxt.toString()))
            nxt = nxt.getNext()


def walk(folder):
    for f in folder.getFiles():
        name = f.getName()
        consumer = state
        try:
            obj = f.getReadOnlyDomainObject(consumer, -1, monitor)
        except Exception as e:
            lines_all.append("program: %s  (FAILED to open: %s)" % (name, e))
            continue
        try:
            scan_program(obj, name)
        finally:
            obj.release(consumer)
    for sub in folder.getFolders():
        walk(sub)


walk(root)

out_path = "%s/jal_allprogs_%08x.txt" % (OUT_DIR, TARGET)
with open(out_path, "w") as fh:
    fh.write("\n".join(lines_all))
print("\n".join(lines_all))
print("--- full report -> %s" % out_path)
