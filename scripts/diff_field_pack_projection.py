#!/usr/bin/env python3
"""
diff_field_pack_projection.py

Diff a captured runtime RAM window (from
scripts/pcsx-redux/autorun_field_pack_projection.lua) against the
on-disc PROT bytes for the same scene's field-pack entry.

The field-pack format is documented at docs/formats/field-pack.md. The
loader (FUN_8001F7C0) transforms the on-disc preamble into a runtime
structure that mixes GP0-shaped GPU primitive packets, the asset
descriptor table, and the asset region. Reading the post-load runtime
cell at base+0x60 (where on-disc slot 0 sits) shows GP0 packets, NOT
the on-disc record bytes - so a single save state can't reveal the
projection. The probe captures the runtime RAM window after the loader
returns; this tool compares it byte-by-byte against the disc bytes
the loader read from.

Output is a per-slot summary keyed by the canonical 97-slot field-pack
schema, listing for each slot:
  * bytes_total
  * bytes_changed (post-load RAM != on-disc PROT)
  * first_diff_offset (relative to slot start)
  * a hex preview of the first differing 32 bytes on each side

USAGE
    python3 scripts/diff_field_pack_projection.py \\
        --runtime-bin /tmp/fp_proj.post.00.bin \\
        --runtime-meta /tmp/fp_proj.post.00.bin.meta \\
        --disc-bytes extracted/prot/0003_town01.bin

The runtime .bin is a raw RAM slice; the .meta sidecar carries the
absolute load address (`base=`) so we know where in the slice the
field-pack window starts. The disc-bytes file is the LZS-decoded
asset bytes for the matching PROT entry (use `lzs-decode` or
`asset extract` to produce it).
"""

import argparse
import struct
import sys
from pathlib import Path
from typing import Optional

# Mirror crates/asset/src/field_pack.rs::CANONICAL_SCHEMA. Byte-identical
# across every field-pack file (MD5 edcfdf1575889d63d2077c396089d7f3),
# anchored on slots[0]==0x60 and slots[96]==0x16651.
#
# Generated programmatically here from the on-disc bytes of the loaded
# disc-bytes file: we read the magic at +0xN and parse the 97 u32 LE
# offsets that follow it. That keeps the script independent of the
# Rust crate and avoids hard-coding 97 numbers.
FIELD_PACK_MAGIC = 0x01059B84
FIELD_PACK_SLOT_COUNT = 97


def find_magic(buf: bytes) -> Optional[int]:
    """Locate the 4-byte field-pack magic in `buf`. Returns absolute offset."""
    needle = struct.pack("<I", FIELD_PACK_MAGIC)
    return buf.find(needle) if needle in buf else None


def read_schema(buf: bytes, magic_off: int) -> list[tuple[int, int]]:
    """Return [(slot_off, slot_size)] for the 97 schema slots."""
    schema_start = magic_off + 4
    schema_end = schema_start + FIELD_PACK_SLOT_COUNT * 4
    if len(buf) < schema_end:
        raise ValueError(
            f"buffer too small for schema (need {schema_end} bytes, got {len(buf)})"
        )
    raw = struct.unpack(
        f"<{FIELD_PACK_SLOT_COUNT}I", buf[schema_start:schema_end]
    )
    out = []
    for i in range(FIELD_PACK_SLOT_COUNT - 1):
        out.append((raw[i], raw[i + 1] - raw[i]))
    out.append((raw[-1], 0))
    return out


def hex_preview(buf: bytes, off: int, count: int = 32) -> str:
    end = min(off + count, len(buf))
    return buf[off:end].hex()


def parse_meta(meta_path: Path) -> dict[str, int | str]:
    out: dict[str, int | str] = {}
    for line in meta_path.read_text().splitlines():
        line = line.strip()
        if "=" not in line:
            continue
        k, _, v = line.partition("=")
        if v.startswith("0x"):
            out[k] = int(v, 16)
        elif v.startswith('"') and v.endswith('"'):
            out[k] = v[1:-1]
        else:
            try:
                out[k] = int(v)
            except ValueError:
                out[k] = v
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--runtime-bin", required=True, type=Path,
                    help="raw RAM slice from the probe (.post.NN.bin)")
    ap.add_argument("--runtime-meta", required=True, type=Path,
                    help="meta sidecar with lo= / base= (.post.NN.bin.meta)")
    ap.add_argument("--disc-bytes", required=True, type=Path,
                    help="on-disc field-pack bytes (LZS-decoded PROT entry)")
    ap.add_argument("--max-print", type=int, default=20,
                    help="max number of slots to print in the diff table")
    ap.add_argument("--all-slots", action="store_true",
                    help="print every differing slot, not just the first N")
    args = ap.parse_args()

    runtime = args.runtime_bin.read_bytes()
    meta = parse_meta(args.runtime_meta)
    lo = meta.get("lo")
    base = meta.get("base")
    if not isinstance(lo, int) or not isinstance(base, int):
        sys.stderr.write(
            "meta sidecar missing lo= or base= (or non-integer values)\n"
        )
        return 2
    runtime_field_off = base - lo
    if runtime_field_off < 0 or runtime_field_off >= len(runtime):
        sys.stderr.write(
            f"runtime base 0x{base:08X} not inside captured slice "
            f"(lo=0x{lo:08X}, len={len(runtime)})\n"
        )
        return 2

    disc = args.disc_bytes.read_bytes()
    disc_magic = find_magic(disc)
    if disc_magic is None:
        sys.stderr.write(
            "field-pack magic 0x01059B84 not found in disc bytes; "
            "is this an LZS-decoded field-pack entry?\n"
        )
        return 2
    schema = read_schema(disc, disc_magic)

    print(f"runtime: {args.runtime_bin}")
    print(f"  capture lo  = 0x{lo:08X}")
    print(f"  field-pack base = 0x{base:08X}  (offset 0x{runtime_field_off:X} "
          f"in slice; slice len {len(runtime)} bytes)")
    print(f"  scene_slot0 = {meta.get('scene_slot0', '?')}")
    print(f"  scene_slot1 = {meta.get('scene_slot1', '?')}")
    print()
    print(f"disc bytes: {args.disc_bytes}")
    print(f"  field-pack magic at offset 0x{disc_magic:X}")
    print(f"  schema starts at 0x{disc_magic + 4:X}")
    print(f"  total disc bytes = {len(disc)}")
    print()

    # The disc-side field-pack region is everything from `magic_off + 4
    # + 97*4` through end-of-asset-region. The runtime base is the start
    # of the runtime layout (the schema's base). To compare them we
    # walk slot-by-slot.
    disc_data_base = disc_magic + 4 + FIELD_PACK_SLOT_COUNT * 4
    print(f"disc data base (after schema) = 0x{disc_data_base:X}")
    print()

    rows = []
    total_changed = 0
    total_bytes = 0
    for slot_idx, (slot_off, slot_size) in enumerate(schema):
        if slot_size == 0:
            continue
        # Disc side: [disc_data_base + slot_off .. + slot_size]. But the
        # schema offsets are referenced from the start of the field-pack
        # data region, which sits just past the schema in some entries
        # and at offset 0 in entry 0005_town01. The runtime layout uses
        # base+slot_off uniformly. Try the post-schema region first.
        disc_slot_start = disc_data_base + slot_off
        disc_slot_end = disc_slot_start + slot_size
        if disc_slot_end > len(disc):
            # Fall back to base-relative (entry 0005_town01 case).
            disc_slot_start = slot_off
            disc_slot_end = slot_off + slot_size
        if disc_slot_end > len(disc):
            continue

        runtime_slot_start = runtime_field_off + slot_off
        runtime_slot_end = runtime_slot_start + slot_size
        if runtime_slot_end > len(runtime):
            continue

        disc_chunk = disc[disc_slot_start:disc_slot_end]
        runtime_chunk = runtime[runtime_slot_start:runtime_slot_end]
        changed = sum(a != b for a, b in zip(disc_chunk, runtime_chunk))
        total_changed += changed
        total_bytes += slot_size
        first_diff = next(
            (i for i, (a, b) in enumerate(zip(disc_chunk, runtime_chunk))
             if a != b),
            None,
        )
        rows.append((slot_idx, slot_off, slot_size, changed, first_diff,
                     disc_chunk, runtime_chunk))

    rows.sort(key=lambda r: -r[3])
    limit = len(rows) if args.all_slots else min(args.max_print, len(rows))

    print("=== per-slot diff (runtime vs on-disc, sorted by changed bytes) ===")
    print(f"{'slot':>4} {'off':>8} {'size':>6} {'chgd':>6} {'first':>6}  preview")
    for row in rows[:limit]:
        idx, off, size, chgd, first, disc_chunk, runtime_chunk = row
        first_s = "-" if first is None else f"+0x{first:X}"
        print(f"{idx:>4} 0x{off:06X} {size:>6} {chgd:>6} {first_s:>6}")
        if first is not None:
            preview_off = max(0, first - 4)
            preview_off &= ~0xF  # 16-byte align
            print(f"      disc    @+0x{preview_off:04X}: "
                  f"{hex_preview(disc_chunk, preview_off)}")
            print(f"      runtime @+0x{preview_off:04X}: "
                  f"{hex_preview(runtime_chunk, preview_off)}")
    if len(rows) > limit:
        print(f"... ({len(rows) - limit} more slots; pass --all-slots)")

    print()
    print("=== summary ===")
    print(f"total bytes compared: {total_bytes}")
    print(f"total bytes changed:  {total_changed} "
          f"({100.0 * total_changed / max(1, total_bytes):.1f}%)")
    unchanged_slots = sum(1 for r in rows if r[3] == 0)
    print(f"slots unchanged:      {unchanged_slots} / {len(rows)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
