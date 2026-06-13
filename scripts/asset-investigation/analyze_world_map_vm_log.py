#!/usr/bin/env python3
"""Summarise a `world_map_vm_calls.csv` produced by the PCSX-Redux Lua hook
in `scripts/pcsx-redux/log_world_map_vm.lua`.

Three questions this answers:

  1. Where does the bytecode live? Histogram of `a1_bytecode_pc` aligned to
     64 KB pages. A handful of dominant pages = static buffer; a wide
     spread = scratch / per-frame allocation.

  2. Which opcodes does the world-map view actually invoke? Counts per
     `sub_op` byte, with the four continent-render opcodes
     (0x2B/0x2C/0x2D/0x2E) flagged.

  3. What's the per-frame draw program? For each `0x2C` (draw continent
     region) call, prints the 5 arg halfwords decoded from the captured
     bytes - those are the slab descriptor inputs the 3D viewer needs.

Usage:

    python3 scripts/asset-investigation/analyze_world_map_vm_log.py world_map_vm_calls.csv
"""

from __future__ import annotations

import argparse
import csv
import struct
import sys
from collections import Counter

# Canonical advance counts (halfwords) per sub-opcode. Mirror of
# `legaia_engine_vm::world_map_draw_vm::canonical_size`.
SIZES = {
    0x00: 16, 0x01: 2, 0x02: 2, 0x03: 2, 0x04: 3, 0x05: 5, 0x06: 7, 0x07: 7,
    0x08: 2, 0x09: 2, 0x0A: 3, 0x0B: 3, 0x0C: 3, 0x0D: 3, 0x0E: 11, 0x0F: 2,
    0x10: 2, 0x11: 2, 0x12: 8, 0x13: 4, 0x14: 4, 0x15: 2, 0x16: 2, 0x17: 8,
    0x18: 5, 0x19: 8, 0x1A: 8, 0x1B: 5, 0x1C: 3, 0x1D: 3, 0x1E: 4, 0x1F: 5,
    0x20: 5, 0x21: 5, 0x22: 5, 0x23: 6, 0x24: 8, 0x25: 3, 0x26: 3, 0x27: 3,
    0x28: 5, 0x29: 5, 0x2A: 8, 0x2B: 6, 0x2C: 7, 0x2D: 6, 0x2E: 13, 0x2F: 3,
    0x30: 5, 0x31: 3, 0x32: 3, 0x33: 6, 0x34: 3, 0x35: 3, 0x36: 4, 0x37: 4,
    0x38: 4, 0x39: 4, 0x3A: 3, 0x3B: 4, 0x3C: 6,
}

CONTINENT_OPS = {0x2B: "slab_uv_set", 0x2C: "draw_continent",
                 0x2D: "slab_uv_inc", 0x2E: "gpu_draw_mode"}


def classify_region(addr: int) -> str:
    """Bucket a PSX virtual address into a known region."""
    if 0x80010000 <= addr < 0x80080000:
        return "scus_or_low_kernel"
    if 0x80080000 <= addr < 0x801C0000:
        return "main_heap_area"
    if 0x801C0000 <= addr < 0x80200000:
        return "overlay_or_high_ram"
    if 0x1F800000 <= addr < 0x1F800400:
        return "scratchpad"
    return f"unknown_0x{addr & 0xFFFF0000:08X}"


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("csv", help="world_map_vm_calls.csv from the Lua hook")
    p.add_argument("--top-pages", type=int, default=10,
                   help="How many top a1 pages to show (default 10)")
    p.add_argument("--draws", type=int, default=20,
                   help="How many 0x2C draw_continent calls to dump (default 20)")
    return p.parse_args()


def main() -> int:
    args = parse_args()

    pages: Counter[int] = Counter()
    ops: Counter[int]   = Counter()
    draws: list[tuple[int, int, list[int]]] = []
    regions: Counter[str] = Counter()

    with open(args.csv, newline="") as fh:
        for row in csv.DictReader(fh):
            a1 = int(row["a1_bytecode_pc"], 16)
            op = int(row["sub_op"], 16)
            raw = bytes.fromhex(row["bytes_hex"])

            pages[a1 & 0xFFFF0000] += 1
            ops[op] += 1
            regions[classify_region(a1)] += 1

            if op == 0x2C:
                # 5 arg halfwords at +4..+0xE
                if len(raw) >= 0xE:
                    a = list(struct.unpack_from("<5H", raw, 4))
                    draws.append((int(row["call_idx"]), a1, a))

    print(f"=== file: {args.csv} ===\n")

    total = sum(ops.values())
    print(f"Total calls: {total}\n")

    print("--- a1 bytecode-PC region distribution ---")
    for region, n in regions.most_common():
        pct = 100.0 * n / total
        print(f"  {region:<24} {n:6d}  ({pct:5.1f}%)")
    print()

    print(f"--- top {args.top_pages} 64KB pages (where the bytecode lives) ---")
    for page, n in pages.most_common(args.top_pages):
        print(f"  0x{page:08X}  {n:6d} calls")
    print()

    print("--- opcode histogram ---")
    for op, n in sorted(ops.items()):
        size = SIZES.get(op, "?")
        marker = f"  <- {CONTINENT_OPS[op]}" if op in CONTINENT_OPS else ""
        print(f"  op 0x{op:02X}  size={size!s:>2}  {n:6d} calls{marker}")
    print()

    if draws:
        print(f"--- first {min(args.draws, len(draws))} draw_continent (0x2C) calls ---")
        print("(args are 5 u16 halfwords from the slab descriptor at op+4)")
        for call_idx, a1, a in draws[:args.draws]:
            args_hex = " ".join(f"0x{x:04X}" for x in a)
            print(f"  call {call_idx:5d}  a1=0x{a1:08X}  args=[{args_hex}]")
    else:
        print("--- no draw_continent (0x2C) calls in this log ---")
        print("If you're sure the world map was rendering during capture,")
        print("the draw goes through a different path - check the opcode")
        print("histogram above for 0x16 (the most common control-script op).")

    return 0


if __name__ == "__main__":
    sys.exit(main())
