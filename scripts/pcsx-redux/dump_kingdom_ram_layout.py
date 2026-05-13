#!/usr/bin/env python3
"""
dump_kingdom_ram_layout.py

Decode each slot of a kingdom's 7-asset bundle from disc, then find
each decompressed slot's location in a PCSX-Redux save state's RAM.
This pins the complete in-RAM layout of the kingdom data and lets us
identify what RAM regions are NOT covered by the kingdom bundle - i.e.
the unknown source(s) of the bulk-continent geometry.

USAGE
    python3 scripts/pcsx-redux/dump_kingdom_ram_layout.py \\
        <save_state> [--bundle map01|map02|map03]
"""

import argparse
import struct
import sys
from pathlib import Path

# Reuse the LZS + RAM-extract helpers.
sys.path.insert(0, str(Path(__file__).parent))
from match_prim_groups_to_disc import (  # noqa: E402
    extract_ram,
    find_asset_table,
    lzs_decompress,
)


def find_in_ram(ram: bytes, needle: bytes) -> int:
    """Return PSX address (0x80000000+offset) of first match, or -1."""
    pos = ram.find(needle)
    return 0x80000000 + pos if pos >= 0 else -1


def slot_label(typ: int) -> str:
    return {
        0x01: "TIM_LIST",
        0x02: "TMD_pack",
        0x03: "MAN_pack",
        0x04: "small_index",
        0x05: "type5_pack",
        0x06: "type6_idx",
        0x07: "type7_idx",
    }.get(typ, f"type_{typ:#04x}")


KINGDOM_BASE = {"map01": 85, "map02": 244, "map03": 391}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("state", help="PCSX-Redux save state path")
    ap.add_argument(
        "--bundle",
        default="map01",
        choices=sorted(KINGDOM_BASE.keys()),
        help="Kingdom bundle to inspect (default: map01).",
    )
    ap.add_argument(
        "--extracted",
        default="extracted",
        help="Extracted disc root (default: extracted/).",
    )
    args = ap.parse_args()

    ram = extract_ram(args.state)
    print(f"loaded {len(ram):#x} bytes of RAM\n")

    # Find the kingdom's PROT entry. try base then base+1, like the runtime
    # loader does.
    base = KINGDOM_BASE[args.bundle]
    candidate_paths = []
    for off in [0, 1]:
        idx = base + off
        files = list(Path(args.extracted, "PROT").glob(f"{idx:04d}_*.BIN"))
        if files:
            candidate_paths.append(files[0])
    chosen = None
    table_off = None
    for p in candidate_paths:
        b = p.read_bytes()
        t = find_asset_table(b)
        if t is not None:
            chosen = p
            table_off = t
            break
    if chosen is None:
        print(f"!! no asset table found in {args.bundle} (base {base})")
        return 1

    raw = chosen.read_bytes()
    table = raw[table_off:]
    print(f"bundle: {chosen.name}  table@{table_off:#x}\n")

    # Decode each slot and find it in RAM
    print(
        f"  {'slot':4s} {'type':5s} {'size':>8s} {'decoded':>8s} {'RAM_start':>12s}"
        f"  {'name':<14s}"
    )
    print("  " + "-" * 64)

    slot_ranges = []
    for k in range(7):
        ts = struct.unpack("<I", table[8 + k * 8 : 12 + k * 8])[0]
        do = struct.unpack("<I", table[12 + k * 8 : 16 + k * 8])[0]
        sz = ts & 0xFFFFFF
        typ = ts >> 24
        if sz == 0:
            continue
        try:
            decoded = lzs_decompress(table[do:], sz)
        except Exception as e:
            print(f"  {k:<4d} {typ:#04x}  {sz:>8d}  FAIL: {e}")
            continue

        # Sample multiple windows in the decoded payload to find its RAM
        # location reliably. Bodies often start at offset 0 (where header
        # matches across many entries), so use middle samples.
        # Drop the result with the LOWEST PSX address that's repeatable -
        # if multiple samples agree, that's the load base.
        addrs = []
        for so in (
            len(decoded) // 4,
            len(decoded) // 2,
            3 * len(decoded) // 4,
        ):
            if so + 64 > len(decoded):
                continue
            sample = bytes(decoded[so : so + 64])
            pos = ram.find(sample)
            if pos >= 0:
                addrs.append(0x80000000 + pos - so)

        # Pick the most-common base (mode); fall back to the first.
        if addrs:
            from collections import Counter

            base_addr = Counter(addrs).most_common(1)[0][0]
            ram_end = base_addr + len(decoded)
            slot_ranges.append((base_addr, ram_end, k, typ, decoded))
            print(
                f"  {k:<4d} {typ:#04x}  {sz:>8d}  {len(decoded):>8d}  "
                f"{base_addr:#012x}  {slot_label(typ):<14s}"
            )
        else:
            print(
                f"  {k:<4d} {typ:#04x}  {sz:>8d}  {len(decoded):>8d}  "
                f"{'(NOT FOUND)':>12s}  {slot_label(typ):<14s}"
            )

    # Sort slots by RAM start address and report contiguous layout
    slot_ranges.sort()
    print(f"\n=== Kingdom RAM layout (sorted by address) ===")
    for start, end, k, typ, _decoded in slot_ranges:
        print(
            f"  {start:#012x}..{end:#012x}  size={end-start:>8d}  "
            f"slot {k} ({slot_label(typ)})"
        )

    # Map the OTHER TMDs (extras beyond the kingdom bundle)
    if slot_ranges:
        kingdom_end = max(end for _, end, *_ in slot_ranges)
        print(f"\n=== TMD magics in RAM AFTER kingdom end ({kingdom_end:#x}) ===")
        TMD_MAGIC = b"\x02\x00\x00\x80"
        off = kingdom_end - 0x80000000
        end = 0x1C0000
        extra_tmds = []
        while off < end - 4:
            if ram[off : off + 4] == TMD_MAGIC:
                extra_tmds.append(0x80000000 + off)
            off += 4
        for addr in extra_tmds:
            # Look at obj count to filter junk
            nobj = struct.unpack("<I", ram[addr - 0x80000000 + 8 : addr - 0x80000000 + 12])[0]
            tag = "(plausible TMD)" if 1 <= nobj <= 100 else f"(nobj={nobj}, prob junk)"
            print(f"  {addr:#012x}  {tag}")


if __name__ == "__main__":
    sys.exit(main() or 0)
