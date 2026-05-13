#!/usr/bin/env python3
"""
verify_slot4_in_ram.py

Cross-check decoded slot 4 sub-bodies against live RAM in a PCSX-Redux
save state, to confirm the slot is loaded into the runtime memory at
the expected address (0x8011a624..0x80122454 for Drake's map01) and to
detect any runtime fixups applied to the data.

USAGE
    python3 scripts/pcsx-redux/verify_slot4_in_ram.py \\
        <save_state> [--bundle map01|map02|map03]
"""

import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from match_prim_groups_to_disc import extract_ram, find_asset_table, lzs_decompress  # noqa: E402

PSX_BASE = 0x80000000
KINGDOM_BASE = {"map01": 85, "map02": 244, "map03": 391}


def load_slot4(extracted_dir: Path, bundle: str):
    base = KINGDOM_BASE[bundle]
    for off in (0, 1):
        idx = base + off
        files = list(extracted_dir.joinpath("PROT").glob(f"{idx:04d}_*.BIN"))
        if not files:
            continue
        raw = files[0].read_bytes()
        table_off = find_asset_table(raw)
        if table_off is None:
            continue
        table = raw[table_off:]
        ts = struct.unpack("<I", table[40:44])[0]
        do = struct.unpack("<I", table[44:48])[0]
        slot_size = ts & 0xFFFFFF
        try:
            decoded = lzs_decompress(table[do:], slot_size)
        except Exception:
            continue
        return files[0], decoded
    return None, None


def find_in_ram(ram: bytes, needle: bytes):
    """Return PSX address of first match, or None."""
    pos = ram.find(needle)
    return PSX_BASE + pos if pos >= 0 else None


def diff_blocks(disc: bytes, ram_slice: bytes):
    """Return list of (offset, disc_byte, ram_byte) for differing bytes."""
    diffs = []
    n = min(len(disc), len(ram_slice))
    for i in range(n):
        if disc[i] != ram_slice[i]:
            diffs.append((i, disc[i], ram_slice[i]))
    return diffs


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("state", help="PCSX-Redux save state path")
    ap.add_argument(
        "--bundle",
        default="map01",
        choices=sorted(KINGDOM_BASE.keys()),
        help="Kingdom bundle (default: map01).",
    )
    ap.add_argument(
        "--extracted",
        default="extracted",
        help="Extracted disc root (default: extracted/).",
    )
    args = ap.parse_args()

    ram = extract_ram(args.state)
    print(f"loaded {len(ram):#x} bytes of RAM\n")

    bundle_path, decoded = load_slot4(Path(args.extracted), args.bundle)
    if decoded is None:
        print(f"!! could not load slot 4 for {args.bundle}")
        return 1
    print(f"slot 4 source: {bundle_path.name}  {len(decoded)} bytes decoded\n")

    # Parse outer pack to enumerate bodies.
    count = struct.unpack("<I", decoded[0:4])[0]
    print(f"outer pack: count = {count}")
    byte_offsets = []
    for k in range(count):
        bo = struct.unpack("<I", decoded[4 + 4 * k : 8 + 4 * k])[0]
        byte_offsets.append(bo)
    print(f"byte_offsets = {byte_offsets}\n")

    # Try to locate each body in RAM by finding a unique signature.
    # Use a 64-byte window starting at offset +16 into each body (past
    # the header, into the data region).
    print(
        f"  {'body':>4s} {'disc_off':>8s} {'body_size':>9s} {'ram_addr':>12s}  notes"
    )
    print("  " + "-" * 60)
    locations = []
    for k in range(count):
        body_start = byte_offsets[k]
        body_end = byte_offsets[k + 1] if k + 1 < count else len(decoded)
        body = decoded[body_start:body_end]
        size = len(body)
        if size < 64:
            print(
                f"  {k:>4d} {body_start:#08x} {size:>9d}  ... too small to probe"
            )
            continue
        # Probe at +16 into the body. (Avoid header which might be
        # universal; pick a deeper offset to lower false-match chance.)
        probe_off = min(16, size - 64)
        probe = bytes(body[probe_off : probe_off + 64])
        ram_addr = find_in_ram(ram, probe)
        if ram_addr is None:
            # Try a second probe further in.
            probe2_off = min(size // 2, size - 64)
            probe2 = bytes(body[probe2_off : probe2_off + 64])
            ram_addr = find_in_ram(ram, probe2)
            if ram_addr is None:
                print(
                    f"  {k:>4d} {body_start:#08x} {size:>9d}     (NOT FOUND)"
                )
                continue
            ram_body_start = ram_addr - probe2_off
        else:
            ram_body_start = ram_addr - probe_off
        locations.append((k, body_start, size, ram_body_start, body))
        # Compute byte differences over the body
        ram_off = ram_body_start - PSX_BASE
        if ram_off < 0 or ram_off + size > len(ram):
            print(
                f"  {k:>4d} {body_start:#08x} {size:>9d}  {ram_body_start:#012x}  OOR"
            )
            continue
        ram_slice = ram[ram_off : ram_off + size]
        diffs = diff_blocks(body, ram_slice)
        note = f"diffs={len(diffs)}"
        print(
            f"  {k:>4d} {body_start:#08x} {size:>9d}  {ram_body_start:#012x}  {note}"
        )

    if not locations:
        return 0

    # Report contiguous layout
    locations.sort(key=lambda r: r[3])
    print(f"\n=== Bodies in RAM (sorted by address) ===")
    base = locations[0][3]
    for k, disc_off, size, ram_start, _body in locations:
        delta = ram_start - base
        print(
            f"  body {k:>2d}: {ram_start:#012x}..{ram_start+size:#012x}  "
            f"size={size:>5d}  disc_off={disc_off:#06x}  Δ={delta:#06x}"
        )

    # Compute total RAM footprint
    ram_end = max(r[3] + r[2] for r in locations)
    ram_start = min(r[3] for r in locations)
    print(
        f"\n  Slot 4 occupies RAM {ram_start:#012x}..{ram_end:#012x} "
        f"({ram_end - ram_start} bytes)"
    )

    # If diffs are zero everywhere, slot 4 is loaded verbatim. Otherwise
    # the runtime applies fixups (likely to specific header words).
    total_diffs = sum(
        len(diff_blocks(body, ram[r[3] - PSX_BASE : r[3] - PSX_BASE + r[2]]))
        for r in locations
        for body in [r[4]]
    )
    print(f"  Total per-byte diffs vs disc: {total_diffs}")
    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
