# @category Legaia
# @runtime Jython
#
# Dump the field-VM 0x4C dispatcher jump tables so the collision-grid
# paint handler (writes _DAT_1f8003ec + 0x4000, code around 0x801e1d00)
# can be matched to its exact outer-nibble / sub-op coordinate.
#
#   0x801E00F4 - main field-VM dispatcher JT
#   0x801CEE60 - 0x4C outer-nibble JT (16 entries)
#
# Run: -process overlay_0897.bin.0 -noanalysis -postScript /scripts/dump_4c_jumptables.py

import os

prog = currentProgram
mem = prog.getMemory()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()

OUT = "/scripts/funcs/overlay_0897_4c_jumptables.txt"


def read_words(base_str, n):
    base = af.getAddress(base_str)
    if mem.getBlock(base) is None:
        return None
    out = []
    for i in range(n):
        a = base.add(i * 4)
        try:
            w = mem.getInt(a) & 0xFFFFFFFF
        except Exception:
            w = None
        out.append((i, a, w))
    return out


def func_at(word):
    if word is None:
        return "?"
    a = af.getAddress("0x%08x" % word)
    f = fm.getFunctionContaining(a)
    if f is None:
        return "(no func)"
    return "{} (+0x{:x})".format(f.getName(),
                                 word - f.getEntryPoint().getOffset())


if mem.getBlock(af.getAddress("801cee60")) is None:
    print("[skip] not overlay_0897")
else:
    fh = open(OUT, "w")
    try:
        for name, base, n in [("MAIN_JT_801E00F4", "801e00f4", 64),
                              ("OP4C_NIBBLE_JT_801CEE60", "801cee60", 16)]:
            fh.write("=== {} ===\n".format(name))
            rows = read_words(base, n)
            for i, a, w in rows:
                tgt = "0x%08x" % w if w is not None else "?"
                fh.write("  [{:2d}] {} -> {}  {}\n".format(
                    i, a, tgt, func_at(w)))
            fh.write("\n")
    finally:
        fh.close()
    print("wrote {}".format(OUT))
print("done")
