# @category Legaia
# @runtime Jython
#
# Dump byte ranges of the overlay program. Targets:
#   - 0x801CED70: 13-entry jump table for FUN_801d6628 opcode dispatch
#   - 0x801E473C: per-opcode operand table (16-byte stride per FUN_801d6628)

import os

prog = currentProgram
af = prog.getAddressFactory()
mem = prog.getMemory()

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

TARGETS = [
    (0x801CED70, 13 * 4, "overlay_jump_table_801CED70_FUN_801d6628_opcodes"),
    (0x801E473C, 0x100, "overlay_operand_table_801E473C"),
    (0x801CE000, 0x60,  "overlay_first_bytes_801CE000"),
    (0x801D6628, 0x10,  "overlay_FUN_801d6628_entry_bytes"),
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
