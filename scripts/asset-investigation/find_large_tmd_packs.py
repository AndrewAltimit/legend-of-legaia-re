#!/usr/bin/env python3
"""
find_large_tmd_packs.py

Bulk-scan every PROT entry for embedded Legaia TMD packs. For each
entry:

1. Try to LZS-decode the whole entry at offset 0 as a single stream
   of unknown size (cap at 1 MB) - covers entries that ARE the
   compressed payload directly.
2. Sweep 0x800-aligned offsets looking for the 7-asset descriptor
   table (`u32 count == 7 && desc[0].data_offset == 0x40`). For each
   slot, LZS-decode its payload using the size declared in
   `desc[i].type_size & 0xFFFFFF`.
3. Count `0x80000002` (Legaia TMD magic) occurrences in each
   decompressed payload. Report entries with >=MIN_HITS magics.

Goal: locate the on-disc source of the ~46-70 world-map continent
TMDs that load into RAM at 0x80125000+ during MAPDSIP. Earlier
hypothesis (PROT base+8 of each kingdom) was wrong - that slot holds
arena/town interiors, not continent terrain.

USAGE
    python3 scripts/asset-investigation/find_large_tmd_packs.py [--min HITS] \\
        [--extracted PATH]
"""

import argparse
import struct
import sys
from pathlib import Path


TMD_MAGIC = b"\x02\x00\x00\x80"


def lzs_decompress(src, expected_size, hard_cap=None):
    """Legaia LZS decoder. If expected_size is None, runs until EOF or
    `hard_cap` bytes - whichever comes first."""
    if expected_size is None and hard_cap is None:
        hard_cap = 1 << 20  # 1 MB default ceiling
    if expected_size is None:
        expected_size = hard_cap

    out = bytearray()
    ring = bytearray(4096)
    rpos = 0xFEE
    src_pos = 0
    flags = 0
    flag_mask = 0
    while len(out) < expected_size:
        if flag_mask == 0:
            if src_pos >= len(src):
                break
            flags = src[src_pos]
            src_pos += 1
            flag_mask = 1
        if flags & flag_mask:
            if src_pos >= len(src):
                break
            b = src[src_pos]
            src_pos += 1
            out.append(b)
            ring[rpos] = b
            rpos = (rpos + 1) & 0xFFF
        else:
            if src_pos + 1 >= len(src):
                break
            lo = src[src_pos]
            hi = src[src_pos + 1]
            src_pos += 2
            offset = lo | ((hi & 0xF0) << 4)
            length = (hi & 0x0F) + 3
            for _ in range(length):
                if len(out) >= expected_size:
                    break
                b = ring[offset & 0xFFF]
                out.append(b)
                ring[rpos] = b
                rpos = (rpos + 1) & 0xFFF
                offset += 1
        flag_mask = (flag_mask << 1) & 0xFF
    return bytes(out)


def count_tmd_magics(buf):
    """Count word-aligned occurrences of the Legaia TMD magic."""
    n = 0
    for i in range(0, len(buf) - 3, 4):
        if buf[i : i + 4] == TMD_MAGIC:
            n += 1
    return n


def find_asset_tables(buf):
    """Yield every 0x800-aligned offset where `[u32 count=7][u32 ?]
    [u32 type_size][u32 data_offset=0x40]` looks like a 7-asset
    descriptor."""
    off = 0
    found = []
    while off + 64 <= len(buf):
        count = struct.unpack("<I", buf[off : off + 4])[0]
        if count == 7:
            d0 = struct.unpack("<I", buf[off + 12 : off + 16])[0]
            if d0 == 0x40:
                found.append(off)
        off += 0x800
    return found


def scan_entry(path, hits_threshold):
    """Inspect one PROT entry. Returns a list of result dicts (one per
    LZS source that decodes to >= hits_threshold TMDs)."""
    raw = path.read_bytes()
    findings = []

    # Strategy 1: also count TMD magics in the entry's RAW bytes. Catches
    # entries that ship uncompressed TMDs at known offsets (e.g. some
    # battle-data or scene-tmd-stream containers).
    n_raw = count_tmd_magics(raw)
    if n_raw >= hits_threshold:
        findings.append(
            dict(
                source="raw",
                table_off=None,
                slot_idx=None,
                decoded_size=len(raw),
                tmd_count=n_raw,
            )
        )

    # Strategy 2: walk every 7-asset table at 0x800-aligned offsets.
    # Descriptor layout: [u32 count][u32 meta][(u32 type_size, u32 data_offset) x 7]
    # so slot k's type_size is at byte offset 8 + k*8, data_offset at 12 + k*8.
    for table_off in find_asset_tables(raw):
        table = raw[table_off:]
        for slot_idx in range(7):
            ts_row = 8 + slot_idx * 8
            do_row = 12 + slot_idx * 8
            if do_row + 4 > len(table):
                break
            try:
                ts = struct.unpack("<I", table[ts_row : ts_row + 4])[0]
                off = struct.unpack("<I", table[do_row : do_row + 4])[0]
            except struct.error:
                break
            slot_type = ts >> 24
            slot_size = ts & 0xFFFFFF
            if slot_type == 0 or slot_size == 0:
                continue
            if off + 1 >= len(table):
                continue
            try:
                decoded = lzs_decompress(table[off:], slot_size)
            except Exception:
                continue
            if len(decoded) < 4:
                continue
            n = count_tmd_magics(decoded)
            if n >= hits_threshold:
                findings.append(
                    dict(
                        source=f"table@0x{table_off:X}/slot{slot_idx}_type0x{slot_type:02X}",
                        table_off=table_off,
                        slot_idx=slot_idx,
                        decoded_size=len(decoded),
                        tmd_count=n,
                    )
                )

    return findings


def load_cdname(cdname_path):
    """Return idx -> label map. Each #define name N starts a block;
    names propagate forward until the next #define."""
    if not cdname_path.exists():
        return {}
    by_idx = {}
    pairs = []
    for ln in cdname_path.read_text().splitlines():
        s = ln.strip()
        if not s.startswith("#define"):
            continue
        parts = s.split()
        if len(parts) < 3:
            continue
        try:
            pairs.append((int(parts[2]), parts[1]))
        except ValueError:
            continue
    pairs.sort()
    for i, (start, name) in enumerate(pairs):
        end = pairs[i + 1][0] - 1 if i + 1 < len(pairs) else 99_999
        for idx in range(start, end + 1):
            by_idx[idx] = name
    return by_idx


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--extracted",
        default="extracted",
        help="Extracted disc root (default: extracted/).",
    )
    ap.add_argument(
        "--min",
        type=int,
        default=20,
        help="Minimum TMD-magic count to report (default: 20).",
    )
    args = ap.parse_args()

    prot_dir = Path(args.extracted) / "PROT"
    cdname = load_cdname(Path(args.extracted) / "CDNAME.TXT")

    files = sorted(prot_dir.glob("[0-9][0-9][0-9][0-9]_*.BIN"))
    print(f"scanning {len(files)} PROT entries with min hits = {args.min} ...\n")

    rows = []
    for f in files:
        try:
            idx = int(f.name[:4])
        except ValueError:
            continue
        findings = scan_entry(f, args.min)
        for fd in findings:
            rows.append(dict(idx=idx, name=f.name, label=cdname.get(idx, "?"), **fd))
        if idx % 100 == 0:
            print(f"  ... at entry {idx}, {len(rows)} hits so far", file=sys.stderr)

    print(
        f"\n{'idx':>5s} {'label':<14s} {'source':<32s} "
        f"{'decoded_size':>12s} {'TMDs':>5s}"
    )
    print("-" * 80)
    # Sort by TMD count descending so the fattest packs surface first.
    rows.sort(key=lambda r: (-r["tmd_count"], r["idx"]))
    for r in rows:
        print(
            f"  {r['idx']:04d} {r['label']:<14s} {r['source']:<32s} "
            f"{r['decoded_size']:>12d} {r['tmd_count']:>5d}"
        )

    print(f"\nTotal: {len(rows)} TMD-rich payloads (>= {args.min} TMDs each).")


if __name__ == "__main__":
    sys.exit(main() or 0)
