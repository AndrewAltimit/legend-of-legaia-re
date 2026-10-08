# @category Legaia
# @runtime Jython
#
# Spine flag-writer sweep (wave 7 arc 1b).
#
# Hunts the un-recovered writers of story flags 0x142 (dolk-dungeon-clear)
# and 0x482 (Drake mist walls), the Zeto battle-id write (DAT_8007b7fc),
# and any extra FMV-id (_DAT_8007BA78) writer. Per program it reports:
#
#   1. every `jal` to the flag setters FUN_8003CE08 (SET) / FUN_8003CE34
#      (CLEAR) with 13 instructions of before-context (a0 provenance) and
#      6 of after-context;
#   2. every LUI-tracked addiu/ori/load/store whose effective address lands
#      in the flag bank 0x80085758..0x80085958, on 0x8007B7FC (battle id)
#      or on 0x8007BA78 (FMV id);
#   3. every `li reg, 0x142` / `li reg, 0x482` immediate (addiu/ori from
#      $zero included);
#   4. every initialized-memory LE data word equal to a setter entry point
#      (function-pointer tables the jal sweep can't see).
#
# Run against every program in the project:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process -noanalysis \
#       -postScript /scripts/find_spine_flag_writers.py
#
# Appends to /scripts/spine_sweep.log (delete it before a fresh run).

from collections import deque

prog = currentProgram
listing = prog.getListing()
fm = prog.getFunctionManager()
mem = prog.getMemory()
pname = prog.getName()

OUT = "/scripts/spine_sweep.log"

SETTERS = {
    0x8003CE08: "FLAG_SET(8003CE08)",
    0x8003CE34: "FLAG_CLEAR(8003CE34)",
}
RANGES = [
    (0x80085758, 0x80085958, "FLAG_BANK"),
    (0x8007B7FC, 0x8007B800, "BATTLE_ID_8007B7FC"),
    (0x8007BA78, 0x8007BA7C, "FMV_ID_8007BA78"),
]
IMMS = {0x142: "IMM_0x142(322)", 0x482: "IMM_0x482(1154)"}

MEM_MNEMS = ("sw", "sh", "sb", "lw", "lh", "lhu", "lb", "lbu", "lwl", "lwr", "swl", "swr")

lines = []


def funclabel(addr):
    f = fm.getFunctionContaining(addr)
    if f is None:
        return "<nofunc>"
    return "%s@%s" % (f.getName(), f.getEntryPoint())


def sext16(v):
    v = v & 0xFFFF
    if v >= 0x8000:
        v -= 0x10000
    return v


def scalar_and_reg(objs):
    sc = None
    rg = None
    for o in objs:
        cn = o.getClass().getSimpleName()
        if cn == "Scalar":
            sc = o.getValue()
        elif cn == "Register":
            rg = o.getName()
    return sc, rg


ctx = deque(maxlen=13)
after_counters = []  # list of [remaining, tag]

last_lui = {}
cur_func = None
n_insn = 0
n_hits = 0

it = listing.getInstructions(True)
while it.hasNext():
    insn = it.next()
    n_insn += 1
    a = insn.getAddress()
    f = fm.getFunctionContaining(a)
    fe = f.getEntryPoint().getOffset() if f else None
    if fe != cur_func:
        last_lui = {}
        cur_func = fe
    txt = "    %s  %s" % (a, insn)

    # flush after-context
    if after_counters:
        for c in after_counters:
            c[0] -= 1
        lines.append(txt)
        after_counters = [c for c in after_counters if c[0] > 0]
        if not after_counters:
            lines.append("")

    hit_tags = []
    mnem = insn.getMnemonicString().lstrip("_")
    nops = insn.getNumOperands()

    if mnem == "jal":
        flows = insn.getFlows()
        for fl in flows:
            off = fl.getOffset()
            if off in SETTERS:
                hit_tags.append("CALL " + SETTERS[off])
    elif mnem == "lui" and nops == 2:
        try:
            reg = insn.getRegister(0)
            sc, _ = scalar_and_reg(insn.getOpObjects(1))
            if reg is not None and sc is not None:
                last_lui[reg.getName()] = (sc & 0xFFFF) << 16
        except:
            pass
    elif mnem in ("addiu", "ori") and nops == 3:
        try:
            src = insn.getRegister(1)
            sc, _ = scalar_and_reg(insn.getOpObjects(2))
            if src is not None and sc is not None:
                srcn = src.getName()
                if srcn == "zero":
                    v = sc & 0xFFFF if mnem == "ori" else sext16(sc) & 0xFFFFFFFF
                    if v in IMMS:
                        hit_tags.append("LOAD " + IMMS[v])
                elif srcn in last_lui:
                    if mnem == "ori":
                        eff = last_lui[srcn] | (sc & 0xFFFF)
                    else:
                        eff = (last_lui[srcn] + sext16(sc)) & 0xFFFFFFFF
                    for lo, hi, tag in RANGES:
                        if lo <= eff < hi:
                            hit_tags.append("ADDR %s -> 0x%08X (%s)" % (tag, eff, mnem))
                    # keep tracking: reg now holds full addr
                    dst = insn.getRegister(0)
                    if dst is not None and dst.getName() == srcn:
                        last_lui[srcn] = eff & 0xFFFF0000
        except:
            pass
    elif mnem == "li" and nops == 2:
        try:
            sc, _ = scalar_and_reg(insn.getOpObjects(1))
            if sc is not None and (sc & 0xFFFFFFFF) in IMMS:
                hit_tags.append("LOAD " + IMMS[sc & 0xFFFFFFFF])
        except:
            pass
    if mnem in MEM_MNEMS and nops == 2:
        try:
            sc, rg = scalar_and_reg(insn.getOpObjects(1))
            if rg is not None and rg in last_lui:
                off = sext16(sc) if sc is not None else 0
                eff = (last_lui[rg] + off) & 0xFFFFFFFF
                for lo, hi, tag in RANGES:
                    if lo <= eff < hi:
                        kind = "STORE" if mnem.startswith("s") else "LOAD"
                        hit_tags.append("%s %s -> 0x%08X (%s)" % (kind, tag, eff, mnem))
        except:
            pass

    if hit_tags:
        n_hits += 1
        lines.append("HIT [%s] at %s in %s" % ("; ".join(hit_tags), a, funclabel(a)))
        for c in ctx:
            lines.append(c)
        lines.append("  >>%s" % txt[4:])
        after_counters.append([6, "x"])

    ctx.append(txt)

# ---- data-word scan for setter addresses (function-pointer tables) ----
targets = {}
for off, nm in SETTERS.items():
    targets[off] = nm
data_hits = []
for blk in mem.getBlocks():
    if not blk.isInitialized():
        continue
    size = blk.getSize()
    start = blk.getStart()
    CHUNK = 0x10000
    pos = 0
    import jarray
    while pos < size:
        n = min(CHUNK, size - pos)
        buf = jarray.zeros(n, "b")
        try:
            blk.getBytes(start.add(pos), buf)
        except:
            break
        # scan aligned LE u32
        base_off = start.getOffset() + pos
        align = (4 - (base_off & 3)) & 3
        i = align
        while i + 4 <= n:
            w = (
                (buf[i] & 0xFF)
                | ((buf[i + 1] & 0xFF) << 8)
                | ((buf[i + 2] & 0xFF) << 16)
                | ((buf[i + 3] & 0xFF) << 24)
            )
            if w in targets:
                va = base_off + i
                data_hits.append("DATA-WORD %s at 0x%08X (%s)" % (targets[w], va, funclabel(start.add(pos + i))))
            i += 4
        pos += n

hdr = "=== PROGRAM %s : %d instructions, %d code hits, %d data-word hits ===" % (
    pname,
    n_insn,
    n_hits,
    len(data_hits),
)
out = open(OUT, "a")
out.write(hdr + "\n")
for l in lines:
    out.write(l + "\n")
for l in data_hits:
    out.write(l + "\n")
out.write("\n")
out.close()
print(hdr)
