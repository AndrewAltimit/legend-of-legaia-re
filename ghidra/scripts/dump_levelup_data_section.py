# @category Legaia
# @runtime Jython
#
# Dumps the data section of overlay_magic_level_up.bin.
# The overlay window is 0x801C0000-0x801FFFFF (256 KB).
# Code functions top out around 0x801EFFE4; data lives above and interspersed.
#
# Key data addresses to probe (from decompiled FUN_801d388c and siblings):
#   0x801f4b80..0x801f4cff  -- small lookup tables (spell-slot indices, type maps)
#   0x801f4b98              -- pointer written to DAT_80076d2c (base of stat table)
#   0x801f6960..0x801f69e0  -- globals written by FUN_801eed1c (magic-level-up state)
#   0x801c8f00..0x801c9400  -- character pointer array + Seru pointer tables
#   0x801f5cf8, 0x801f5d90  -- text-data pointers passed to func_0x80050ed4
#   0x801f0000..0x801f4b7f  -- unknown; candidate for per-character growth tables
#
# Run against:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_magic_level_up.bin -noanalysis \
#       -postScript /scripts/dump_levelup_data_section.py
#
# Output: /scripts/funcs/overlay_magic_level_up_data_<region>.txt

import os

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
af = prog.getAddressFactory()
mem = prog.getMemory()
listing = prog.getListing()
fm = prog.getFunctionManager()

REGIONS = [
    # name, start, end (inclusive)
    ("data_0x801c8f00", 0x801c8f00, 0x801c93ff),  # char/Seru ptr arrays
    ("data_0x801f0000", 0x801f0000, 0x801f0fff),  # candidate growth tables block 1
    ("data_0x801f1000", 0x801f1000, 0x801f1fff),  # block 2
    ("data_0x801f2000", 0x801f2000, 0x801f2fff),  # block 3
    ("data_0x801f3000", 0x801f3000, 0x801f3fff),  # block 4
    ("data_0x801f4000", 0x801f4000, 0x801f4fff),  # block 5 (known lookup tables)
    ("data_0x801f5000", 0x801f5000, 0x801f5fff),  # block 6 (text data pointers)
    ("data_0x801f6000", 0x801f6000, 0x801f6fff),  # block 7 (magic-level-up state vars)
    ("data_0x801f7000", 0x801f7000, 0x801f9fff),  # block 8
    ("data_0x801fa000", 0x801fa000, 0x801fffff),  # block 9 top
]


def addr_is_code(addr):
    func = fm.getFunctionContaining(addr)
    return func is not None


def dump_region(name, start_off, end_off):
    try:
        start_addr = af.getAddress("0x{:08x}".format(start_off))
        end_addr = af.getAddress("0x{:08x}".format(end_off))
    except Exception as e:
        print("[skip] bad address range {}: {}".format(name, e))
        return

    block = mem.getBlock(start_addr)
    if block is None or not block.isInitialized():
        print("[skip] no initialized block at {}".format(name))
        return

    out_path = os.path.join(OUT_DIR, "overlay_magic_level_up_{}.txt".format(name))
    fh = open(out_path, "w")
    try:
        fh.write("== {} DATA REGION 0x{:08X}..0x{:08X} [{}] ==\n".format(
            name, start_off, end_off, prog_name))
        fh.write("C=code D=data\n\n")

        addr = start_addr
        while addr.compareTo(end_addr) <= 0:
            off = addr.getOffset()
            is_code = addr_is_code(addr)

            # Read 16 bytes
            buf = bytearray(16)
            n = 0
            cur = addr
            for i in range(16):
                if cur.compareTo(end_addr) > 0:
                    break
                try:
                    b = mem.getByte(cur)
                    buf[i] = b & 0xff
                    n += 1
                except Exception:
                    buf[i] = 0
                    n += 1
                cur = cur.add(1)

            hex_part = " ".join("{:02x}".format(buf[i]) for i in range(n))
            hex_part = hex_part.ljust(47)
            asc_part = "".join(chr(buf[i]) if 0x20 <= buf[i] < 0x7f else "." for i in range(n))
            tag = "C" if is_code else "D"
            fh.write("{} {:08X}: {} {}\n".format(tag, off, hex_part, asc_part))

            try:
                addr = addr.add(16)
            except Exception:
                break

    finally:
        fh.close()
    print("wrote {} ({})".format(out_path, name))


for (name, start, end) in REGIONS:
    dump_region(name, start, end)

print("done data dump [{}]".format(prog_name))
