# @category Legaia
# @runtime Jython
#
# Dump byte ranges of SCUS_942.54 RAM image. Used for inspecting data tables
# that the renderer references (e.g. DAT_8007326c, the per-prim-mode table).

import os

prog = currentProgram
af = prog.getAddressFactory()
mem = prog.getMemory()

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

# (start_addr, length, label)
TARGETS = [
    (0x8007326C, 0x80, "DAT_8007326c_prim_mode_table"),
    (0x80073270, 0x80, "DAT_80073270_prim_mode_table_p4"),
    (0x8007326C - 0x10, 0x100, "around_DAT_8007326c"),
    (0x8007B410, 0x10, "DAT_8007b410_used_by_renderer"),
]

for addr_int, length, label in TARGETS:
    addr = af.getAddress("{:x}".format(addr_int))
    out_path = os.path.join(OUT_DIR, "data_{:08x}_{}.txt".format(addr_int, label))
    with open(out_path, "w") as fh:
        fh.write("== {} {} (len={}) ==\n".format(label, addr, length))
        try:
            buf = bytearray(length)
            for i in range(length):
                buf[i] = mem.getByte(addr.add(i)) & 0xFF
            fh.write("Hex:\n")
            for row in range(0, length, 16):
                line = " ".join("{:02X}".format(buf[row + c]) for c in range(min(16, length - row)))
                fh.write("  +{:04x}: {}\n".format(row, line))
            fh.write("\nAs u32 LE:\n")
            for row in range(0, length, 4):
                if row + 4 <= length:
                    val = buf[row] | (buf[row + 1] << 8) | (buf[row + 2] << 16) | (buf[row + 3] << 24)
                    fh.write("  +{:04x}: 0x{:08X}\n".format(row, val))
        except Exception as e:
            fh.write("(error: {})\n".format(e))
    print("wrote {}".format(out_path))
print("done")
