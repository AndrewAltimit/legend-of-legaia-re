# @category Legaia
# @runtime Jython
#
# Find the writer of the field/world-map actor's mesh-chain pointer at
# `actor+0x44`. The per-actor render dispatcher FUN_8001ADA4 case 5 reads
# `*(actor+0x44)` as a chain `[count, &obj0, &obj1, ...]` where each entry is
# `pool_tmd + 0xc + obj*0x1c` (the Legaia-TMD object-header stride). The .MAP
# grid placer FUN_8003A55C spawns the actor with `+0x44 == 0` (FUN_80020e3c
# zeroes it), so a SEPARATE function builds the chain and stores its address to
# `actor+0x44`. That function is the mesh-per-object resolver for the walk view
# (it selects which DAT_8007C018 pool TMD an object draws).
#
# Strategy: scan every instruction for `sw/sh rX, 0x44(rY)` with rY not sp/gp
# (a struct store, not a stack slot). Rank the containing function by whether it
# also (a) references the global pool table DAT_8007C018 (0x8007C018), (b) does
# 0x1c-stride math (the TMD object stride: sll/mul producing *0x1c, or an addiu
# 0x1c / 0xc), and (c) reads actor fields used by the placer (+0x56 mode, +0x60
# record idx, +0x5c). Dump the top candidates to /scripts/funcs/<addr>.txt.
#
# ASCII-only (Jython 2.7). Run against SCUS_942.54 first (the field render +
# placer code is SCUS-resident); re-run against an overlay if no SCUS hit:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process SCUS_942.54 -noanalysis \
#     -scriptPath /scripts -postScript find_mesh_chain_writer.py

import os

from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass


def addr(off):
    return af.getDefaultAddressSpace().getAddress(off)


POOL_TABLE = 0x8007C018  # DAT_8007C018 global asset-pointer table

# ---------------------------------------------------------------------------
# 1. Collect `sw/sh rX, 0x44(rY)` stores with a non-stack base register.
# ---------------------------------------------------------------------------
print("=== 1. scanning for sw/sh ...,0x44(reg) struct stores ===")
hits = []  # (insn_addr, func_entry, base_reg)
it = listing.getInstructions(True)
while it.hasNext():
    insn = it.next()
    mnem = insn.getMnemonicString()
    if mnem not in ("sw", "sh"):
        continue
    if insn.getNumOperands() != 2:
        continue
    try:
        objs = insn.getOpObjects(1)
        off_v = None
        base = None
        for o in objs:
            cn = o.getClass().getSimpleName()
            if cn == "Scalar":
                off_v = o.getValue()
            elif cn == "Register":
                base = o.toString()
        if off_v != 0x44 or base in (None, "sp", "gp", "s8"):
            continue
        func = fm.getFunctionContaining(insn.getAddress())
        fe = func.getEntryPoint().getOffset() if func else None
        hits.append((insn.getAddress(), fe, base))
    except Exception:
        pass

print("  %d struct stores to +0x44 found" % len(hits))


# ---------------------------------------------------------------------------
# 2. Score each containing function: pool-table ref, 0x1c-stride math, actor
#    field reads (+0x56/+0x5c/+0x60). FUN_80020e3c (init zeroing) is excluded.
# ---------------------------------------------------------------------------
def func_features(func):
    """Return (refs_pool, has_1c_stride, reads_5x6x, stores_nonzero_to_44)."""
    refs_pool = False
    has_1c = False
    reads_fld = False
    nonzero_44 = False
    last_lui = {}
    it2 = listing.getInstructions(func.getBody(), True)
    while it2.hasNext():
        insn = it2.next()
        mnem = insn.getMnemonicString()
        # pool-table reference via lui+addiu
        if mnem == "lui" and insn.getNumOperands() == 2:
            try:
                reg = insn.getDefaultOperandRepresentation(0)
                imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
                last_lui[reg] = imm << 16
            except Exception:
                pass
        if mnem in ("addiu", "ori") and insn.getNumOperands() == 3:
            try:
                b = insn.getDefaultOperandRepresentation(1)
                lo = insn.getOpObjects(2)[0].getValue() & 0xFFFF
                if b in last_lui:
                    eff = (last_lui[b] + lo) & 0xFFFFFFFF
                    if POOL_TABLE - 0x40 <= eff <= POOL_TABLE + 0x40:
                        refs_pool = True
                # addiu producing 0x1c or 0xc (TMD object stride / base)
                lo_scalar = insn.getOpObjects(2)[0].getValue()
                if lo_scalar in (0x1c, 0xc):
                    has_1c = True
            except Exception:
                pass
        # immediate 0x1c anywhere (sll by ... or li): catch `sll ,,N`+`addu` *28
        if mnem in ("sll",) and insn.getNumOperands() == 3:
            try:
                sh = insn.getOpObjects(2)[0].getValue()
                # *28 is built as (x<<4)+(x<<3)+(x<<2) or (x<<5)-(x<<2); flag <<2..<<5 clusters loosely
                if sh in (2, 3, 4, 5):
                    pass  # too noisy alone; ignored
            except Exception:
                pass
        # actor field reads at +0x56/+0x5c/+0x60
        if mnem in ("lhu", "lh", "lw", "lbu", "lb") and insn.getNumOperands() == 2:
            try:
                for o in insn.getOpObjects(1):
                    if o.getClass().getSimpleName() == "Scalar" and o.getValue() in (0x56, 0x5c, 0x60, 0x44):
                        reads_fld = True
            except Exception:
                pass
        # store to +0x44 that is not `sw zero,...`
        if mnem in ("sw", "sh") and insn.getNumOperands() == 2:
            try:
                src = insn.getDefaultOperandRepresentation(0)
                objs = insn.getOpObjects(1)
                off_v = None
                base = None
                for o in objs:
                    cn = o.getClass().getSimpleName()
                    if cn == "Scalar":
                        off_v = o.getValue()
                    elif cn == "Register":
                        base = o.toString()
                if off_v == 0x44 and base not in ("sp", "gp", "s8") and src != "zero":
                    nonzero_44 = True
            except Exception:
                pass
    return refs_pool, has_1c, reads_fld, nonzero_44


print("")
print("=== 2. scoring containing functions ===")
seen = {}
for ia, fe, base in hits:
    if fe is None or fe == 0x80020e3c:  # skip the init-zeroing function
        continue
    seen.setdefault(fe, []).append((ia, base))

scored = []
for fe, sites in seen.items():
    func = fm.getFunctionContaining(addr(fe))
    rp, h1c, rf, nz = func_features(func)
    score = (3 if nz else 0) + (3 if rp else 0) + (1 if h1c else 0) + (1 if rf else 0)
    scored.append((score, fe, rp, h1c, rf, nz, len(sites), sites[0][1]))

scored.sort(reverse=True)
print("  score  func        pool 1c fld nz44 nsites base")
for score, fe, rp, h1c, rf, nz, n, base in scored:
    print("  %4d   0x%08x  %s  %s  %s  %s   %d    %s"
          % (score, fe, "Y" if rp else ".", "Y" if h1c else ".",
             "Y" if rf else ".", "Y" if nz else ".", n, base))


# ---------------------------------------------------------------------------
# 3. Dump the top candidates (disasm + decompiled C).
# ---------------------------------------------------------------------------
decomp = DecompInterface()
decomp.openProgram(prog)
monitor = ConsoleTaskMonitor()


def dump_func(fe):
    func = fm.getFunctionContaining(addr(fe))
    if func is None:
        return
    lines = ["== %s %08x ==" % (func.getName(), fe), "", "--- DISASSEMBLY ---"]
    it3 = listing.getInstructions(func.getBody(), True)
    while it3.hasNext():
        ins = it3.next()
        lines.append("%s  %s" % (ins.getAddress(), ins.toString()))
    lines.append("")
    lines.append("--- DECOMPILED ---")
    try:
        res = decomp.decompileFunction(func, 60, monitor)
        if res is not None and res.decompileCompleted():
            lines.append(res.getDecompiledFunction().getC())
        else:
            lines.append("(decompile failed)")
    except Exception, e:
        lines.append("(decompile exception: %s)" % e)
    p = os.path.join(OUT_DIR, "%08x.txt" % fe)
    f = open(p, "w")
    f.write("\n".join(lines))
    f.close()
    print("  [dump] %s -> %s" % (func.getName(), p))


print("")
print("=== 3. dumping top candidates ===")
for score, fe, rp, h1c, rf, nz, n, base in scored[:6]:
    if score >= 4:
        dump_func(fe)

print("")
print("=== find_mesh_chain_writer done ===")
