# @category Legaia
# @runtime Jython
#
# Trace the per-scene field-file loader FUN_8001f7c0 to pin which disc file
# (PROT entry / ISO path) a scene's `.MAP` comes from -- specifically to
# disambiguate the game-mode-0x03 WALK `.MAP` from the game-mode-0x0D
# OVERVIEW `.MAP` for the kingdom overworld scenes (map01..map03).
#
# FUN_8001f7c0 is a DUAL-MODE loader gated on two globals:
#
#   if (_DAT_8007b868 == 0 && _DAT_8007b8c2 != 0)   // RETAIL
#       FUN_8003e8a8(param_3, 1);   // param_3 = PROT entry index -> in-RAM TOC
#   else                                            // DEV-HOST
#       FUN_8003e6bc("DATA_FIELD\<scene>.MAP", ...) // break 0x103 fopen on PC
#
# RETAIL path: param_3 (the loader's 3rd arg) IS a PROT entry index. The caller
# (field-init FUN_801d6704 @ 0x801d6ae8) passes a2 = *(0x80084540) -- the scene's
# PROT index, the word right before the scene-name string at 0x80084548.
# FUN_8003e8a8 indexes the in-RAM PROT TOC at 0x801c70f0
# (`toc[index+2]` = start_lba; constant appears as addiu -0x7fe38f10 == 0x801c70f0).
# So the entry a scene loads is fully determined by 0x80084540, NOT by any
# extension->offset map: e.g. map01 walk holds 0x80084540 = 0x55 = PROT entry 85
# (= 0085_map01, the CDNAME map01 base), confirmed against a live walk capture.
#
# DEV-HOST path (break 0x103 = FUN_800608f0): a PsyQ host-link `fopen` that opens
# a real `DATA_FIELD\<scene>.MAP` (+`.PCH` at +0x12000, +`\efect.dat`) on the
# developer's PC -- there is NO ISO9660 DATA\FIELD tree on the retail disc and NO
# extension->PROT mapping inside the trap; it is never taken when _DAT_8007b8c2 != 0.
# The scene name is built from 0x80084548; extensions from DAT_8007b3bc/.3c4.
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
    (0x80084540, 0x80084544, "PROT-index global 0x80084540 (retail param_3)"),
    (0x8007b868, 0x8007b86c, "dev-host gate _DAT_8007b868 (==0 for retail)"),
    (0x8007b8c2, 0x8007b8c4, "PROT-index gate _DAT_8007b8c2 (!=0 for retail)"),
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


# ---------------------------------------------------------------------------
# 5. Confirm FUN_8003e8a8 (the retail resolver) indexes the in-RAM PROT TOC at
#    0x801c70f0: scan its addiu/lui immediates for the TOC base constant. This
#    is what proves param_3 (= 0x80084540) is a PROT entry index, so the entry a
#    scene loads is pinned by that global -- no break-0x103 extension map exists.
# ---------------------------------------------------------------------------
print("")
print("=== 5. FUN_8003e8a8 PROT-TOC base (proves param_3 = PROT index) ===")
TOC_BASE = 0x801c70f0
try:
    f8a8 = fm.getFunctionContaining(addr(0x8003e8a8))
    found_toc = False
    if f8a8 is not None:
        ii = listing.getInstructions(f8a8.getBody(), True)
        last_lui = {}
        while ii.hasNext():
            insn = ii.next()
            mnem = insn.getMnemonicString()
            if mnem == "lui" and insn.getNumOperands() == 2:
                reg = insn.getDefaultOperandRepresentation(0)
                imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
                last_lui[reg] = imm << 16
            elif mnem == "addiu" and insn.getNumOperands() == 3:
                base = insn.getDefaultOperandRepresentation(1)
                lo = insn.getOpObjects(2)[0].getValue()
                if base in last_lui:
                    eff = (last_lui[base] + lo) & 0xFFFFFFFF
                    if eff == TOC_BASE:
                        print("  %s : addiu -> 0x%08x == in-RAM PROT TOC base" % (insn.getAddress(), eff))
                        found_toc = True
    if not found_toc:
        print("  (TOC base 0x801c70f0 not seen as a direct addiu; check decompiled C")
        print("   for `(param_1 + N) * 4 + -0x7fe38f10` -- that -0x7fe38f10 == 0x801c70f0)")
except Exception, e:
    print("  (scan error: %s)" % e)

print("")
print("=== trace_field_loader done ===")
