# @category Legaia
# @runtime Jython
#
# Trace the per-scene field-file loader FUN_8001f7c0 to pin which disc file
# (PROT entry / ISO path) a scene's `.MAP` comes from -- specifically to
# disambiguate the game-mode-0x03 WALK `.MAP` from the game-mode-0x0D
# OVERVIEW `.MAP` for the kingdom overworld scenes (map01..map03).
#
# FUN_8001f7c0 builds a filename `DATA_FIELD\<scene><ext>` (scene name from the
# global at 0x80084548) and loads it into the field buffer base
# (`_DAT_1f8003ec`, passed as param_1) via the dual-mode opener
# (FUN_8003e6bc -> name resolve; FUN_800608f0 -> CD open). The per-scene file
# EXTENSION is read from the globals DAT_8007b3bc / DAT_8007b3c4, and a second
# file at +0x12000 plus efect.dat are loaded after. Whoever writes the
# extension globals (per scene / per game-mode) selects the walk vs overview
# file, so that is the discriminator this trace pins.
#
# Output: prints to console + writes /scripts/funcs/<addr>.txt for each dumped
# function. ASCII-only (Jython 2.7).
#
# Run inside the container against SCUS_942.54:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process SCUS_942.54 -noanalysis \
#     -postScript /scripts/trace_field_loader.py

import os

from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
mem = prog.getMemory()
ref_mgr = prog.getReferenceManager()

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass


def addr(off):
    return af.getDefaultAddressSpace().getAddress(off)


# ---------------------------------------------------------------------------
# 1. Dump the loader chain (disasm + decompiled C) to /scripts/funcs/<addr>.txt
# ---------------------------------------------------------------------------
CHAIN = [
    0x8001f7c0,  # the field-file loader itself (entry)
    0x8003e6bc,  # load-file-by-name -> in-RAM buffer (name resolver)
    0x800608f0,  # CD file open by path
    0x80060910,  # CD read continuation
    0x8003e8a8,  # debug PROT-index address resolver (alt path)
    0x8003e800,  # debug PROT-index loader (alt path)
    0x80056758,  # string-builder init (copies 0x80084548 scene name)
    0x80056728,  # string-builder append
]

decomp = DecompInterface()
decomp.openProgram(prog)
monitor = ConsoleTaskMonitor()


def dump_func(off):
    a = addr(off)
    func = fm.getFunctionContaining(a)
    if func is None:
        print("  [dump] no function at 0x%08x (not in this program)" % off)
        return
    entry = func.getEntryPoint()
    lines = []
    body = func.getBody()
    n_insn = 0
    ii = listing.getInstructions(body, True)
    lines.append("== %s %08x (entry=%08x) ==" % (func.getName(), entry.getOffset(), entry.getOffset()))
    lines.append("")
    lines.append("--- DISASSEMBLY ---")
    while ii.hasNext():
        insn = ii.next()
        lines.append("%s  %s" % (insn.getAddress(), insn.toString()))
        n_insn += 1
    lines.append("")
    lines.append("--- DECOMPILED ---")
    try:
        res = decomp.decompileFunction(func, 60, monitor)
        if res is not None and res.decompileCompleted():
            lines.append(res.getDecompiledFunction().getC())
        else:
            lines.append("(decompile failed)")
    except Exception, e:  # noqa: E999 (Jython 2)
        lines.append("(decompile exception: %s)" % e)
    path = os.path.join(OUT_DIR, "%08x.txt" % entry.getOffset())
    f = open(path, "w")
    f.write("\n".join(lines))
    f.close()
    print("  [dump] %s -> %s (%d insns)" % (func.getName(), path, n_insn))


print("=== 1. dump loader chain ===")
for off in CHAIN:
    dump_func(off)


# ---------------------------------------------------------------------------
# 2. Read the path-template string constants referenced by FUN_8001f7c0
# ---------------------------------------------------------------------------
def read_cstr(off, maxlen=64):
    out = []
    a = addr(off)
    for i in range(maxlen):
        b = mem.getByte(a.add(i)) & 0xFF
        if b == 0:
            break
        out.append(chr(b) if 32 <= b < 127 else "\\x%02x" % b)
    return "".join(out)


print("")
print("=== 2. path-template + extension string constants ===")
for label, off in [
    ("DATA_FIELD template @0x80010490", 0x80010490),
    ("h:\\PROT\\FIELD template @0x800105e0", 0x800105e0),
    ("efect.dat @0x800105f0", 0x800105f0),
    ("ext global DAT_8007b3bc", 0x8007b3bc),
    ("ext global DAT_8007b3c4", 0x8007b3c4),
]:
    try:
        print("  %-36s = %r" % (label, read_cstr(off)))
    except Exception, e:
        print("  %-36s = (read error: %s)" % (label, e))


# ---------------------------------------------------------------------------
# 3. Find LUI+ADDIU / store writers of the extension globals (0x8007b3bc,
#    0x8007b3c4) and the scene-name global (0x80084548). These setters select
#    the walk vs overview file per scene / game-mode.
# ---------------------------------------------------------------------------
WATCH = [
    (0x8007b3bc, 0x8007b3c8, "ext-globals DAT_8007b3bc/c4"),
    (0x80084548, 0x8008454c, "scene-name global 0x80084548"),
    (0x8007b3c4, 0x8007b3c5, "DAT_8007b3c4 (2nd-file ext)"),
]

print("")
print("=== 3. LUI+ADDIU / mem-access writers of ext + scene globals ===")
inst_iter = listing.getInstructions(True)
last_lui = {}
cur_func = None
for w_lo, w_hi, w_label in WATCH:
    pass
hits = []
inst_iter = listing.getInstructions(True)
while inst_iter.hasNext():
    insn = inst_iter.next()
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != cur_func:
        last_lui = {}
        cur_func = fa
    mnem = insn.getMnemonicString()
    if mnem == "lui" and insn.getNumOperands() == 2:
        try:
            reg = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except Exception:
            pass
        continue
    # addiu R, base, lo  -> effective = lui[base] + lo
    if mnem in ("addiu", "ori") and insn.getNumOperands() == 3:
        try:
            base = insn.getDefaultOperandRepresentation(1)
            lo = insn.getOpObjects(2)[0].getValue()
            if base in last_lui:
                eff = (last_lui[base] + (lo & 0xFFFF)) & 0xFFFFFFFF
                for w_lo, w_hi, w_label in WATCH:
                    if w_lo <= eff < w_hi:
                        hits.append((insn.getAddress(), fa, mnem, eff, w_label))
        except Exception:
            pass
    # sw/sh/sb/lw R, off(base) -> effective = lui[base] + off
    if mnem in ("sw", "sh", "sb", "lw", "lhu", "lbu") and insn.getNumOperands() == 2:
        try:
            objs = insn.getOpObjects(1)
            # form: [scalar_off, register]
            off_v = None
            base = None
            for o in objs:
                cn = o.getClass().getSimpleName()
                if cn == "Scalar":
                    off_v = o.getValue()
                elif cn == "Register":
                    base = o.toString()
            if base in last_lui and off_v is not None:
                eff = (last_lui[base] + (off_v & 0xFFFF)) & 0xFFFFFFFF
                for w_lo, w_hi, w_label in WATCH:
                    if w_lo <= eff < w_hi:
                        hits.append((insn.getAddress(), fa, mnem, eff, w_label))
        except Exception:
            pass

if not hits:
    print("  (no LUI+ADDIU/mem hits -- globals may be gp-relative or set via a2/a3 args)")
for a, fa, mnem, eff, lbl in hits:
    fn = "0x%08x" % fa if fa else "(no func)"
    print("  %s in %s : %s -> 0x%08x  [%s]" % (a, fn, mnem, eff, lbl))


# ---------------------------------------------------------------------------
# 4. Callers (xrefs) of FUN_8001f7c0 -- the scene-load entry points that supply
#    param_1 (.MAP buffer base) and param_2 (scene/sub name).
# ---------------------------------------------------------------------------
print("")
print("=== 4. callers (xrefs) of FUN_8001f7c0 ===")
target = addr(0x8001f7c0)
refs = ref_mgr.getReferencesTo(target)
found = False
for r in refs:
    frm = r.getFromAddress()
    cf = fm.getFunctionContaining(frm)
    cfn = ("0x%08x" % cf.getEntryPoint().getOffset()) if cf else "(no func)"
    print("  call from %s in %s (%s)" % (frm, cfn, r.getReferenceType()))
    found = True
if not found:
    print("  (no direct xrefs -- caller likely uses a jr/jalr or LUI+JALR not in ref mgr)")

print("")
print("=== trace_field_loader done ===")
