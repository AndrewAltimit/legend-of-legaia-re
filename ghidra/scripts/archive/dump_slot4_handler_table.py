# @category Legaia
# @runtime Jython
#
# Dump the slot-4 record-kind handler jump tables used by FUN_80043390:
#   SCUS table at 0x8007657C (used when _DAT_1f800394 & 1 == 0)
#   Overlay table at 0x801F8968 (used when _DAT_1f800394 & 1 != 0)
#
# Each entry is a u32 function pointer. Index = (record_word_0 >> 0x11) * 4.

import os

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
mem = prog.getMemory()
af = prog.getAddressFactory()


def dump_table(label, base_str, n):
    base = af.getAddress(base_str)
    if base is None:
        print("[skip] %s base %s not an address" % (label, base_str))
        return None
    lines = []
    lines.append("== %s table @ %s, %d entries ==" % (label, base_str, n))
    for i in range(n):
        a = base.add(i * 4)
        try:
            v = mem.getInt(a) & 0xFFFFFFFF
        except Exception as e:
            lines.append("[%2d] %s : <unreadable: %s>" % (i, a, e))
            continue
        lines.append("[%2d] %s -> 0x%08x" % (i, a, v))
    text = "\n".join(lines) + "\n"
    out_path = os.path.join(OUT_DIR, "slot4_handler_table_" + label + ".txt")
    with open(out_path, "w") as fh:
        fh.write(text)
    print("wrote %s" % out_path)
    return text


# Dispatcher does `s5 = s7 >> 0x11`, so max kind index is 0x7FFF in theory.
# Empirically slot-4 body headers have `kind` field as u16; observed kinds
# in extracted bodies span 1..4. Dump first 64 entries to surface real coverage.
N = 64
dump_table("scus_0x8007657C", "0x8007657C", N)
dump_table("overlay_0x801F8968", "0x801F8968", N)
print("done")
