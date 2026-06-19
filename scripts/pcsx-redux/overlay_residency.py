#!/usr/bin/env python3
"""Byte-match a PROT overlay payload against RAM from a capture.

Answers "is this overlay RESIDENT at its recovered base in this state?"
- the overlay-identity question the static-overlay pipeline cannot
settle on its own (it recovers a self-consistent base, but only a live
RAM image proves which game mode actually loads the entry).

Accepts any of three RAM sources:
  * a PCSX-Redux .sstate (gzipped protobuf; main RAM = field 1 of the
    outer message, 8 MiB - the PSX 2 MiB plus the dev-console mirror);
  * a raw full-RAM dump (2 MiB, e.g. autorun_dump_full_ram.lua output);
  * a window dump with --window-base <va> (e.g. the window_plus*.bin
    files from autorun_minigame_overlay_capture.lua).

Match fraction is computed over NON-ZERO payload bytes only (zero runs
match trivially against BSS). The per-chunk profile separates a real
residency (a contiguous ~1.00 prefix from the base) from over-read
aliasing: consecutive overlay PROT entries carry each other's bytes in
their over-read footprints, so a *suffix* of one entry's payload can
match 1.00 because a DIFFERENT overlay is resident in the next slot
window. Use --split at the next slot's base VA to separate the two
regions (e.g. --split 0x801CE818 when checking PROT 0896 @0x801C5818
against the slot-A window).

Usage:
    overlay_residency.py <ram_source> --prot-file extracted/PROT/0896_bat_back_dat.BIN \
        --base 0x801C5818 [--split 0x801CE818] [--window-base 0x801C0000]
"""

import argparse
import gzip
import sys
from pathlib import Path

PSX_RAM_SIZE = 0x200000
# protobuf tag: field 1, wire-type 2, varint length 0x800000 (8 MiB)
SSTATE_RAM_TAG = bytes([0x0A, 0x80, 0x80, 0x80, 0x04])


def load_ram(path: Path) -> tuple[bytes, int]:
    """Return (ram_bytes, base_va_of_byte_0)."""
    raw = path.read_bytes()
    if raw[:2] == b"\x1f\x8b":
        blob = gzip.decompress(raw)
        p = blob.find(SSTATE_RAM_TAG)
        if p < 0 or p > 0x100:
            raise SystemExit("main-RAM protobuf tag not found in sstate")
        return blob[p + 5 : p + 5 + PSX_RAM_SIZE], 0x80000000
    return raw, 0x80000000


def match_fraction(payload: bytes, ram_region: bytes, lo: int, hi: int):
    nz = m = 0
    for a, b in zip(payload[lo:hi], ram_region[lo:hi]):
        if a == 0:
            continue
        nz += 1
        if a == b:
            m += 1
    return (m / nz if nz else 0.0), m, nz


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("ram_source", type=Path, help=".sstate / full-RAM dump / window dump")
    ap.add_argument("--prot-file", type=Path, required=True, help="as-loaded overlay payload")
    ap.add_argument("--base", type=lambda s: int(s, 0), required=True, help="overlay base VA")
    ap.add_argument(
        "--split",
        type=lambda s: int(s, 0),
        help="VA splitting the unique head from the over-read tail (next slot's base)",
    )
    ap.add_argument(
        "--window-base",
        type=lambda s: int(s, 0),
        help="ram_source is a window dump starting at this VA (raw sources only)",
    )
    ap.add_argument("--chunk", type=lambda s: int(s, 0), default=0x4000, help="profile chunk size")
    args = ap.parse_args()

    payload = args.prot_file.read_bytes()
    ram, ram_base = load_ram(args.ram_source)
    if args.window_base is not None:
        ram_base = args.window_base

    off = args.base - ram_base
    if off < 0 or off >= len(ram):
        raise SystemExit(f"base 0x{args.base:08X} outside RAM source (base 0x{ram_base:08X})")
    region = ram[off : off + len(payload)]
    if len(region) < len(payload):
        print(
            f"note: payload over-reads the RAM source by 0x{len(payload) - len(region):X} bytes; "
            "comparing the in-range prefix"
        )

    f, m, nz = match_fraction(payload, region, 0, len(region))
    print(f"total: {f:.4f} ({m}/{nz} non-zero bytes over 0x{len(region):X})")

    if args.split is not None:
        cut = args.split - args.base
        fh, mh, nh = match_fraction(payload, region, 0, cut)
        ft, mt, nt = match_fraction(payload, region, cut, len(region))
        print(f"head (base..0x{args.split:08X}): {fh:.4f} ({mh}/{nh})")
        print(f"tail (0x{args.split:08X}..):     {ft:.4f} ({mt}/{nt})")

    cells = []
    for s in range(0, len(region), args.chunk):
        fc, _, nc = match_fraction(payload, region, s, s + args.chunk)
        cells.append(f"{fc:.2f}" if nc > 100 else " -- ")
    print(f"profile (per 0x{args.chunk:X}):", " ".join(cells))
    return 0


if __name__ == "__main__":
    sys.exit(main())
