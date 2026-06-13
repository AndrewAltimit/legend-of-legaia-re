#!/usr/bin/env python3
"""
match_title_staging_to_prot.py

Byte-match per-decode source dumps from
`autorun_title_staging_capture.lua` against every PROT entry to pin
each decode's on-disc source.

The probe captures the COMPRESSED bytes the LZS decoder reads (the
contents of the in-RAM staging buffer at the instant FUN_8001A55C
is entered). If the decoded output is something we're hunting for
(e.g. the title overlay), its on-disc source must appear byte-for-
byte inside one of the PROT entries.

For each `decode_NNN_*.bin` this script:
  1. Reads up to FINGERPRINT bytes as a search key.
  2. Scans every `extracted/PROT/NNNN_*.BIN` for that key.
  3. For each hit, verifies by extending the match for as many bytes
     as possible (the captured slice may be a prefix of the on-disc
     entry).
  4. Reports the best match: PROT index + entry name + offset + matched
     bytes / captured bytes.

USAGE
    python3 scripts/asset-investigation/match_title_staging_to_prot.py
        [--captures-dir captures/boot_walk/title_staging]
        [--prot-dir extracted/PROT]
        [--fingerprint 64]
        [--min-match 32]
        [--only-title-range]
"""

import argparse
import os
import sys
from pathlib import Path


TITLE_LO = 0x801C0000
TITLE_HI = 0x801F0000


def parse_decode_filename(name: str):
    # decode_<idx>_src<HEX>_dst<HEX>_len<N>.bin
    if not name.startswith("decode_") or not name.endswith(".bin"):
        return None
    body = name[len("decode_"):-len(".bin")]
    parts = body.split("_")
    if len(parts) != 4:
        return None
    try:
        idx = int(parts[0])
        src = int(parts[1][len("src"):], 16)
        dst = int(parts[2][len("dst"):], 16)
        length = int(parts[3][len("len"):])
    except ValueError:
        return None
    return idx, src, dst, length


def load_prot_corpus(prot_dir: Path):
    entries = []
    for p in sorted(prot_dir.iterdir()):
        if not p.name.endswith(".BIN"):
            continue
        try:
            idx = int(p.name[:4])
        except ValueError:
            continue
        entries.append((idx, p.name, p))
    return entries


def find_matches(prot_entry: bytes, key: bytes):
    """Yield every offset in prot_entry where prot_entry[offset:].startswith(key)."""
    if len(key) == 0:
        return
    start = 0
    while True:
        i = prot_entry.find(key, start)
        if i < 0:
            return
        yield i
        start = i + 1


def extend_match(prot_entry: bytes, offset: int, captured: bytes) -> int:
    """Return the number of contiguous bytes that match between
    prot_entry[offset:] and captured[:]."""
    avail = min(len(captured), len(prot_entry) - offset)
    for i in range(avail):
        if prot_entry[offset + i] != captured[i]:
            return i
    return avail


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--captures-dir", default="captures/boot_walk/title_staging")
    ap.add_argument("--prot-dir", default="extracted/PROT")
    ap.add_argument("--fingerprint", type=int, default=64,
                    help="bytes from the start of each capture used as the search key")
    ap.add_argument("--min-match", type=int, default=32,
                    help="report a match only if at least this many bytes match")
    ap.add_argument("--only-title-range", action="store_true",
                    help="only process decodes whose dst lands in the title-overlay range")
    args = ap.parse_args()

    captures_dir = Path(args.captures_dir)
    prot_dir = Path(args.prot_dir)
    if not captures_dir.is_dir():
        sys.exit(f"no such captures dir: {captures_dir}")
    if not prot_dir.is_dir():
        sys.exit(f"no such PROT dir: {prot_dir}")

    decodes = []
    for p in sorted(captures_dir.iterdir()):
        meta = parse_decode_filename(p.name)
        if meta is None:
            continue
        idx, src, dst, length = meta
        if args.only_title_range and not (TITLE_LO <= dst < TITLE_HI):
            continue
        decodes.append((idx, src, dst, length, p))

    if not decodes:
        sys.exit(f"no decode_*.bin files in {captures_dir}")

    print(f"found {len(decodes)} decode dumps in {captures_dir}")

    # Pre-load the PROT corpus into memory once (~30 MB).
    prot_entries = load_prot_corpus(prot_dir)
    prot_bytes = {}
    for idx, name, path in prot_entries:
        prot_bytes[idx] = (name, path.read_bytes())
    print(f"loaded {len(prot_entries)} PROT entries from {prot_dir}")

    # Search.
    print()
    print(f"{'decode':>6}  {'dst':>10}  {'len':>7}  best-match (PROT entry @ offset, matched bytes)")
    print(f"{'-'*6}  {'-'*10}  {'-'*7}  {'-'*70}")

    for idx, src, dst, length, path in decodes:
        captured = path.read_bytes()
        if len(captured) == 0:
            print(f"{idx:>6}  0x{dst:08X}  {length:>7}  (no captured bytes)")
            continue
        key = captured[: args.fingerprint]
        if len(key) == 0:
            print(f"{idx:>6}  0x{dst:08X}  {length:>7}  (empty key)")
            continue

        best = None  # (matched_bytes, prot_idx, prot_name, prot_offset)
        all_hits = []
        for p_idx in sorted(prot_bytes.keys()):
            name, data = prot_bytes[p_idx]
            for off in find_matches(data, key):
                m = extend_match(data, off, captured)
                if m < args.min_match:
                    continue
                all_hits.append((m, p_idx, name, off))
                if best is None or m > best[0]:
                    best = (m, p_idx, name, off)

        in_title = TITLE_LO <= dst < TITLE_HI
        marker = " [TITLE]" if in_title else ""
        if best is None:
            print(f"{idx:>6}  0x{dst:08X}  {length:>7}  (no match{marker})")
        else:
            m, p_idx, p_name, off = best
            extra = ""
            if len(all_hits) > 1:
                extra = f"  (+{len(all_hits)-1} more hits)"
            print(f"{idx:>6}  0x{dst:08X}  {length:>7}  "
                  f"PROT {p_idx:04d} {p_name} @ 0x{off:X}  "
                  f"matched {m}/{len(captured)} bytes{extra}{marker}")

    print()
    print("done")


if __name__ == "__main__":
    main()
