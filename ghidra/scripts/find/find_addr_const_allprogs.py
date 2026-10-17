# @category Legaia
# @runtime Jython
#
# Sweep EVERY program for occurrences of TARGET as an address constant:
#   (a) a stored 32-bit LE word in initialized memory equal to TARGET
#       (pointer / jump-table entry), and
#   (b) a LUI+ADDIU register-immediate pair that materialises TARGET
#       (LO/HI split: HI=upper16, plus signed LO).
# Output -> /scripts/funcs.

import os

TARGET = 0x801D71F0
OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

# Split for LUI/ADDIU detection: addiu sign-extends LO.
HI = (TARGET >> 16) & 0xFFFF
LO = TARGET & 0xFFFF
if LO & 0x8000:
    HI = (HI + 1) & 0xFFFF  # compensate so lui HI ; addiu (LO as signed) == TARGET

state = getState()
project = state.getProject()
pdata = project.getProjectData()
root = pdata.getRootFolder()

from ghidra.util.task import ConsoleTaskMonitor
monitor = ConsoleTaskMonitor()

lines_all = []
TBYTES = bytearray([TARGET & 0xFF, (TARGET >> 8) & 0xFF, (TARGET >> 16) & 0xFF, (TARGET >> 24) & 0xFF])


def scan_data_words(prog, pname, fm):
    mem = prog.getMemory()
    hits = []
    for block in mem.getBlocks():
        if not block.isInitialized():
            continue
        start = block.getStart()
        size = block.getSize()
        # read in chunks
        buf = jarray_read(mem, start, size)
        if buf is None:
            continue
        n = len(buf)
        i = 0
        while i + 4 <= n:
            if (buf[i] & 0xFF) == TBYTES[0] and (buf[i+1] & 0xFF) == TBYTES[1] and \
               (buf[i+2] & 0xFF) == TBYTES[2] and (buf[i+3] & 0xFF) == TBYTES[3]:
                a = start.add(i)
                func = fm.getFunctionContaining(a)
                fn = func.getName() if func else "-"
                hits.append((str(a), fn))
            i += 1
    return hits


def jarray_read(mem, start, size):
    import jarray
    try:
        b = jarray.zeros(size, "b")
        got = mem.getBytes(start, b)
        return b[:got]
    except Exception:
        return None


def scan_lui_addiu(prog, pname, fm):
    listing = prog.getListing()
    it = listing.getInstructions(True)
    luis = {}  # reg -> (val, addr)
    hits = []
    while it.hasNext():
        insn = it.next()
        m = insn.getMnemonicString()
        if m == "lui":
            try:
                reg = insn.getOpObjects(0)[0]
                imm = insn.getOpObjects(1)[0].getValue()
                luis[str(reg)] = (imm & 0xFFFF, insn.getAddress())
            except Exception:
                pass
        elif m in ("addiu", "ori"):
            try:
                dst = str(insn.getOpObjects(0)[0])
                src = str(insn.getOpObjects(1)[0])
                imm = insn.getOpObjects(2)[0].getValue() & 0xFFFF
                if src in luis:
                    hi = luis[src][0]
                    if m == "addiu":
                        lo = imm if imm < 0x8000 else imm - 0x10000
                        val = ((hi << 16) + lo) & 0xFFFFFFFF
                    else:
                        val = ((hi << 16) | imm) & 0xFFFFFFFF
                    if val == (TARGET & 0xFFFFFFFF):
                        func = fm.getFunctionContaining(insn.getAddress())
                        fn = func.getName() if func else "-"
                        hits.append((str(insn.getAddress()), fn, m, str(luis[src][1])))
            except Exception:
                pass
    return hits


def scan_program(prog, pname):
    fm = prog.getFunctionManager()
    dwords = scan_data_words(prog, pname, fm)
    luis = scan_lui_addiu(prog, pname, fm)
    if dwords or luis:
        lines_all.append("program: %s" % pname)
        for a, fn in dwords:
            lines_all.append("  [DATA word == TARGET] @ %s (in %s)" % (a, fn))
        for a, fn, m, luiaddr in luis:
            lines_all.append("  [%s materialises TARGET] @ %s (in %s) lui@%s" % (m, a, fn, luiaddr))


def walk(folder):
    for f in folder.getFiles():
        name = f.getName()
        try:
            obj = f.getReadOnlyDomainObject(state, -1, monitor)
        except Exception as e:
            lines_all.append("program: %s  (FAILED: %s)" % (name, e))
            continue
        try:
            scan_program(obj, name)
        finally:
            obj.release(state)
    for sub in folder.getFolders():
        walk(sub)


walk(root)
out_path = "%s/addrconst_%08x.txt" % (OUT_DIR, TARGET)
with open(out_path, "w") as fh:
    fh.write("\n".join(lines_all))
print("\n".join(lines_all))
print("--- full report -> %s" % out_path)
