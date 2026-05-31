# @category Legaia
# @runtime Jython
#
# Raw-byte sweep of EVERY program's initialized memory for a MIPS `jal TARGET`
# encoding (catches call sites in regions Ghidra never disassembled). Also
# reports the 4-byte aligned word matches and which (if any) function/block
# they fall in. Output -> /scripts/funcs.

import os
import jarray

TARGET = 0x801D71F0
OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

JAL = 0x0C000000 | ((TARGET >> 2) & 0x03FFFFFF)
JB = [JAL & 0xFF, (JAL >> 8) & 0xFF, (JAL >> 16) & 0xFF, (JAL >> 24) & 0xFF]  # LE

state = getState()
project = state.getProject()
pdata = project.getProjectData()
root = pdata.getRootFolder()

from ghidra.util.task import ConsoleTaskMonitor
monitor = ConsoleTaskMonitor()

lines = []


def scan(prog, pname):
    mem = prog.getMemory()
    fm = prog.getFunctionManager()
    listing = prog.getListing()
    found = []
    for block in mem.getBlocks():
        if not block.isInitialized():
            continue
        start = block.getStart()
        size = int(block.getSize())
        try:
            b = jarray.zeros(size, "b")
            n = mem.getBytes(start, b)
        except Exception:
            continue
        i = 0
        while i + 4 <= n:
            if (b[i] & 0xFF) == JB[0] and (b[i+1] & 0xFF) == JB[1] and \
               (b[i+2] & 0xFF) == JB[2] and (b[i+3] & 0xFF) == JB[3]:
                a = start.add(i)
                func = fm.getFunctionContaining(a)
                fn = func.getName() if func else "-"
                ins = listing.getInstructionAt(a)
                found.append((str(a), fn, ins.toString() if ins else "(not-disassembled)"))
            i += 4
    if found:
        lines.append("program: %s  -- %d jal-encoding match(es)" % (pname, len(found)))
        for a, fn, txt in found:
            lines.append("  @ %s in %s : %s" % (a, fn, txt))


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
lines.insert(0, "jal encoding word = 0x%08X  LE bytes = %s" % (JAL, " ".join("%02x" % x for x in JB)))
out = "%s/jalraw_%08x.txt" % (OUT_DIR, TARGET)
with open(out, "w") as fh:
    fh.write("\n".join(lines))
print("\n".join(lines))
print("--- report -> %s" % out)
