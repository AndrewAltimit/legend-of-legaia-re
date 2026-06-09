#!/usr/bin/env python3
"""
locate_slot4_base.py

Byte-locate a kingdom's slot-4 resident RAM base by searching a post-warp
full main-RAM dump for the disc-decoded slot-4 payload. The base VARIES per
kingdom (Drake/Sebucus/Karisto each load slot-4 to a different address), so a
fixed base can't be assumed; this pins it empirically.

The RAM dump is the raw 2 MiB main RAM written by
`autorun_dump_full_ram_hold.lua` (which drives the held-direction kingdom warp,
then dumps post-warp). The disc payload is the LZS-decoded slot 4 of the
kingdom bundle (PROT 0085/0244/0391 for map01/map02/map03), loaded VERBATIM
into RAM, so every body's bytes appear unchanged.

USAGE
    python3 scripts/pcsx-redux/locate_slot4_base.py \\
        captures/slot4_base/sebucus_ram.bin --bundle map02 [--extracted extracted]

Exit status is non-zero when the payload is not found (warp didn't load slot 4,
or the dump predates the load).
"""

import argparse
import struct
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from verify_slot4_in_ram import PSX_BASE, load_slot4  # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("ram", help="raw main-RAM dump (.bin) from autorun_dump_full_ram_hold.lua")
    ap.add_argument(
        "--bundle",
        default="map01",
        choices=("map01", "map02", "map03"),
        help="kingdom bundle (map01=Drake / map02=Sebucus / map03=Karisto)",
    )
    ap.add_argument("--extracted", default="extracted", help="extracted disc root")
    args = ap.parse_args()

    ram = Path(args.ram).read_bytes()
    bundle_path, decoded = load_slot4(Path(args.extracted), args.bundle)
    if decoded is None:
        print(f"!! could not decode disc slot-4 for {args.bundle}", file=sys.stderr)
        return 2
    count = struct.unpack("<I", decoded[0:4])[0]
    offs = [struct.unpack("<I", decoded[4 + 4 * k : 8 + 4 * k])[0] for k in range(count)]

    # Each body's bytes appear verbatim in RAM; a 64-byte probe at +16 into a
    # body locates it, and (ram_pos - probe_off - body_disc_off) is the outer
    # pack base. A unanimous vote across bodies is the resident base.
    votes: Counter = Counter()
    located = 0
    for k in range(count):
        bs = offs[k]
        be = offs[k + 1] if k + 1 < count else len(decoded)
        body = decoded[bs:be]
        if len(body) < 80:
            continue
        pos = ram.find(body[16 : 16 + 64])
        if pos >= 0:
            votes[pos - 16 - bs] += 1
            located += 1

    print(f"{bundle_path.name}: {len(decoded)} bytes decoded, {count} bodies")
    if not votes:
        print("!! disc slot-4 payload NOT found in RAM (warp didn't load it?)", file=sys.stderr)
        return 1
    base, n = votes.most_common(1)[0]
    print(
        f"slot-4 resident base = {PSX_BASE + base:#010x} "
        f"({n}/{located} located bodies agree); "
        f"window {PSX_BASE + base:#010x}..{PSX_BASE + base + len(decoded):#010x}"
    )
    if len(votes) > 1:
        print("  (other base votes:", {hex(PSX_BASE + b): c for b, c in votes.most_common()[1:]}, ")")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
