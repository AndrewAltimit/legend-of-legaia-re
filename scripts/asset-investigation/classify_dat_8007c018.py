#!/usr/bin/env python3
"""Classify entries in the global asset-pointer table DAT_8007C018.

Reads a 2 MiB main-RAM dump (per `autorun_dump_full_ram.lua`) and walks
the global asset-pointer table at 0x8007C018. For each populated entry,
reads the first 32 bytes the pointer references and classifies them:

  - tmd:        starts with 0x80000002 (Legaia TMD magic)
  - ffafffa:    starts with 0xFFFAFFFA pair (vertex-like; (-6,-6) i16)
  - vertex:     two adjacent int16 fields in [-32K, 32K] non-zero range
  - text:       printable ASCII bytes in the first 4
  - zero:       all-zero leading 32 bytes
  - other:      anything else

The output groups entries by run-length within the table for easy
inspection.

Usage:
    python3 scripts/asset-investigation/classify_dat_8007c018.py captures/ram_dumps/drake_world.bin
"""
import struct
import sys


RAM_BASE = 0x80000000
RAM_SIZE = 2 * 1024 * 1024
TABLE_ADDR = 0x8007C018
TABLE_LEN = 256  # entries (each entry = u32 pointer)


def main(path: str) -> int:
    with open(path, "rb") as fh:
        ram = fh.read()
    if len(ram) < RAM_SIZE:
        print(f"WARN: ram dump is {len(ram)} bytes, expected {RAM_SIZE}", file=sys.stderr)

    def read_u32_at(virt_addr: int) -> int | None:
        off = virt_addr - RAM_BASE
        if off < 0 or off + 4 > len(ram):
            return None
        return struct.unpack_from("<I", ram, off)[0]

    def read_at(virt_addr: int, n: int) -> bytes | None:
        off = virt_addr - RAM_BASE
        if off < 0 or off + n > len(ram):
            return None
        return ram[off : off + n]

    def classify(buf: bytes | None) -> str:
        if buf is None:
            return "BAD_PTR"
        # Treat all-zero leading 16 bytes as zero-padded
        if all(b == 0 for b in buf[:16]):
            return "zero"
        w0 = struct.unpack_from("<I", buf, 0)[0]
        if w0 == 0x80000002:
            return "tmd"
        if w0 == 0xFFFAFFFA:
            return "ffafffa"
        # Try int16 pair: two reasonable signed-i16 fields, neither zero
        i16_0, i16_1 = struct.unpack_from("<hh", buf, 0)
        if -10000 <= i16_0 <= 10000 and -10000 <= i16_1 <= 10000 \
           and not (i16_0 == 0 and i16_1 == 0):
            return "i16pair"
        # Detect printable ASCII (e.g. text fragment)
        if all(32 <= b < 127 or b in (0, 9, 10, 13) for b in buf[:8]):
            printable = sum(1 for b in buf[:8] if 32 <= b < 127)
            if printable >= 4:
                return "text"
        # Many fixed-pattern bytes — texture-like (4bpp index runs etc.)
        # Heuristic: low entropy in low nibble
        return "other"

    print(f"classifying entries at {TABLE_ADDR:08X} (up to {TABLE_LEN}):")
    print()

    entries = []
    for i in range(TABLE_LEN):
        ent_addr = TABLE_ADDR + i * 4
        ptr = read_u32_at(ent_addr)
        if ptr is None or ptr == 0:
            entries.append((i, ptr or 0, "null", None))
            continue
        if not (RAM_BASE <= ptr < RAM_BASE + RAM_SIZE):
            entries.append((i, ptr, "extern", None))
            continue
        buf = read_at(ptr, 32)
        cls = classify(buf)
        entries.append((i, ptr, cls, buf))

    # Group consecutive same-class entries
    def group_runs():
        run_start = 0
        run_cls = entries[0][2]
        for i in range(1, len(entries)):
            if entries[i][2] != run_cls:
                yield (run_start, i - 1, run_cls)
                run_start = i
                run_cls = entries[i][2]
        yield (run_start, len(entries) - 1, run_cls)

    print(f"{'IDX':>4}..{'IDX':>4} {'CLS':<8} count  example_pointer")
    for lo, hi, cls in group_runs():
        first = entries[lo]
        print(f"{lo:4d}..{hi:4d}  {cls:<8} {hi-lo+1:>4}   "
              f"@0x{first[1]:08X}")

    print()
    print(f"=== detail dump for first non-null entry in each class ===")
    seen = set()
    for i, ptr, cls, buf in entries:
        if cls in seen or cls in ("null", "BAD_PTR", "extern"):
            continue
        seen.add(cls)
        print()
        print(f"[{i:3d}] cls={cls}  ptr=0x{ptr:08X}")
        if buf:
            hexs = " ".join(f"{b:02X}" for b in buf)
            print(f"      hex: {hexs}")

    print()
    print("=== entries [45..63] with full 32 bytes ===")
    for i in range(45, 64):
        idx, ptr, cls, buf = entries[i]
        line = f"[{i:3d}] ptr=0x{ptr:08X} cls={cls}"
        if buf:
            hexs = " ".join(f"{b:02X}" for b in buf[:16])
            line += f"  {hexs}"
        print(line)

    print()
    print("=== entries [114..193] with full 16 bytes ===")
    for i in range(114, 194):
        idx, ptr, cls, buf = entries[i]
        line = f"[{i:3d}] ptr=0x{ptr:08X} cls={cls:<8}"
        if buf:
            hexs = " ".join(f"{b:02X}" for b in buf[:16])
            line += f"  {hexs}"
        print(line)

    print()
    counters = {}
    for _, _, cls, _ in entries:
        counters[cls] = counters.get(cls, 0) + 1
    print(f"=== class totals ===")
    for k, v in sorted(counters.items(), key=lambda kv: -kv[1]):
        print(f"  {k:<10} {v}")

    # also dump the slot-4 body offsets for indices 94..113 (slot-4 path)
    print()
    print("=== entries [94..113] (slot-4 body-aligned per prior trace) ===")
    SLOT4_BASE = 0x8011A624
    SLOT4_END = 0x80122454
    for i in range(94, 114):
        idx, ptr, cls, buf = entries[i]
        offset_in_slot4 = ptr - SLOT4_BASE if SLOT4_BASE <= ptr < SLOT4_END else None
        suf = f" slot4+0x{offset_in_slot4:04X}" if offset_in_slot4 is not None else " (NOT in slot-4 range)"
        line = f"[{i:3d}] ptr=0x{ptr:08X} cls={cls:<8}{suf}"
        if buf:
            hexs = " ".join(f"{b:02X}" for b in buf[:16])
            line += f"  {hexs}"
        print(line)

    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(main(sys.argv[1]))
