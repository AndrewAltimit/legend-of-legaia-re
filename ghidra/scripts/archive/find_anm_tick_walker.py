# @category Legaia
# @runtime Jython
#
# Find candidate ANM per-frame walker functions: any function that reads
# from offsets +0x4C (anm_pc), +0x56 (anm_state byte), and +0x68 (anm_timer
# u16) of the same base register within its body.
#
# `FUN_80024CFC` writes those three slots when starting an animation; the
# walker that reads them every frame is overlay-resident and undocumented.

OFFSETS = (0x4C, 0x56, 0x68)

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()

print("[anm-walker] scanning {} for offsets {}".format(prog.getName(), OFFSETS))


def func_reads_anm_offsets(func):
    """Return the set of OFFSETS the function loads from any base register
    via lw/lh/lhu/lb/lbu instructions. We don't track the base register --
    just look for any load that quotes one of the candidate offsets in
    the displacement.
    """
    found = set()
    for ins in listing.getInstructions(func.getBody(), True):
        mnem = ins.getMnemonicString().lower()
        if mnem not in ("lw", "lh", "lhu", "lb", "lbu", "sw", "sh", "sb"):
            continue
        # MIPS load/store form: <mnem> reg, offset(base)
        # Operand 1 is the displacement+base; we just scan the textual rep
        # for "<offset>(...)" and "-<offset>(...)" matches.
        text = ins.toString().lower()
        for off in OFFSETS:
            tag = "0x{:x}(".format(off)
            tag2 = "{:d}(".format(off)
            if tag in text or tag2 in text:
                found.add(off)
    return found


hits = []
for f in fm.getFunctions(True):
    found = func_reads_anm_offsets(f)
    if len(found) >= 2:
        hits.append((f.getName(), f.getEntryPoint(), found))

print("[anm-walker] candidates with >=2 of the actor anm offsets: {}".format(len(hits)))
hits.sort(key=lambda h: -len(h[2]))
for name, addr, found in hits[:30]:
    print("  {} @ {}  : {}".format(
        name,
        addr,
        ",".join("0x{:X}".format(o) for o in sorted(found)),
    ))

print("[anm-walker] done")
