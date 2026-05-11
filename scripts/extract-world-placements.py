#!/usr/bin/env python3
"""Extract per-kingdom world-map placements (the "Man" asset) and write them
to site/world-overview.json for the site's world-overview page.

The Man asset lives at slot index 2 (type byte 0x03) in each kingdom's
`scene_scripted_asset_table` PROT entry. The full asset bundle's LZS stream
is keyed at the asset-table base + slot's descriptor offset. After
decompression, FUN_8003A1E4 reads placement records starting at offset
0x22 of the decompressed Man buffer.

Records per kingdom (b + c iterable, indices a+1..total-1):
- Drake  (PROT 85, scene `map01`): a=12, b=9, c=42 -> 50 placements
- Sebucus(PROT 244, scene `map02`): a=33, b=8, c=16 -> 23 placements
- Karisto(PROT 391, scene `map03`): a=25, b=20, c=21 -> 40 placements

Per-record format (verbatim from the FUN_8003A1E4 walker at 0x8003A2B0):
  u8 n_chars          ; count of 2-byte Shift-JIS name characters
  u8[2 * n_chars]     ; Shift-JIS encoded name
  u8 tmd_slot         ; scene-relative TMD pool index
                      ; if < 0xF0: idx = slot + scene_TMD_base
                      ; if >=0xF0: idx = slot - 0xF0 + global_TMD_base
                      ;            (global "story-character" overlay TMDs)
  u8 flag             ; high nibble seems to encode actor category
  u8 x_enc, z_enc     ; ((b & 0x7F) << 7) + 0x80/0x40 (sign-flag in MSB)

Records with both x_enc and z_enc set to 0x7F have NO static world
position - those actors are positioned by the field-VM script at runtime
(party members, story characters, system actors). The site only renders
placements with real world coordinates.

See memory/project_world_map_render_state.md for the full reverse-engineering
notes (resolver chain + format spec). Run from the repo root:

    python3 scripts/extract-world-placements.py \\
        --prot-dir /tmp/legaia-extract/PROT \\
        --out site/world-overview.json
"""
from __future__ import annotations
import argparse
import glob
import json
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

KINGDOMS = [
    # prot_base, key,       label,             cdname
    (85,  "drake",   "Drake Kingdom",   "map01"),
    (244, "sebucus", "Sebucus Islands", "map02"),
    (391, "karisto", "Karisto Kingdom", "map03"),
]

# Map flag high nibble -> the actor-list bucket used by the site for coloring.
# Empirically: 0x50 placements are scene landmarks (towns / dungeons),
# 0x20-0x2F are entities (event NPCs, separated party members), 0x60-0x70
# tend to be special / scripted triggers, 0x00 covers misc. The exact
# semantics are an open RE item; this mapping is just a visual grouping.
def flag_to_list(flag: int) -> int:
    hi = flag & 0xF0
    if hi == 0x50:
        return 2  # background
    if hi in (0x20, 0x30, 0x40):
        return 1  # entities
    if hi in (0x60, 0x70):
        return 6  # extra
    return 0      # player/system


def find_asset_table(buf: bytes) -> int | None:
    """Locate the 7-asset table within a scene_scripted_asset_table PROT
    entry. The asset table sits at a 0x800-aligned offset past the prescript;
    detect by `u32 count = 7` and `descriptor[0].data_offset = 0x40`."""
    for off in range(0, len(buf), 0x800):
        if off + 64 > len(buf):
            break
        if struct.unpack_from("<I", buf, off)[0] != 7:
            continue
        if struct.unpack_from("<I", buf, off + 12)[0] == 0x40:
            return off
    return None


def lzs_decompress(lzs_bin: Path, src: bytes, decompressed_size: int) -> bytes:
    """Decompress an LZS payload via the lzs-decode CLI (target/release/lzs-decode)."""
    with tempfile.NamedTemporaryFile(delete=False) as src_f:
        src_f.write(src)
        src_path = src_f.name
    with tempfile.NamedTemporaryFile(delete=False) as dst_f:
        dst_path = dst_f.name
    try:
        r = subprocess.run(
            [str(lzs_bin), "raw", src_path, "--size", str(decompressed_size), "--output", dst_path],
            capture_output=True, text=True, check=True,
        )
        return Path(dst_path).read_bytes()
    finally:
        Path(src_path).unlink(missing_ok=True)
        Path(dst_path).unlink(missing_ok=True)


def extract_man(prot_path: Path, lzs_bin: Path) -> bytes:
    buf = prot_path.read_bytes()
    table = find_asset_table(buf)
    if table is None:
        raise RuntimeError(f"no 7-asset table found in {prot_path}")
    # Slot 2 = Man (type byte 0x03)
    type_size = struct.unpack_from("<I", buf, table + 8 + 2 * 8)[0]
    offset = struct.unpack_from("<I", buf, table + 8 + 2 * 8 + 4)[0]
    type_byte = type_size >> 24
    size = type_size & 0xFF_FF_FF
    if type_byte != 0x03:
        raise RuntimeError(
            f"slot 2 type is 0x{type_byte:02X}, expected 0x03 (Man) in {prot_path}"
        )
    src_start = table + offset
    src = buf[src_start:]
    return lzs_decompress(lzs_bin, src, size)


def parse_placements(man: bytes) -> dict:
    hdr = 0x22
    a, b, c = struct.unpack_from("<3h", man, hdr)
    total = a + b + c
    off_tbl = hdr + 9
    offsets = [
        man[off_tbl + i * 3]
        | (man[off_tbl + i * 3 + 1] << 8)
        | (man[off_tbl + i * 3 + 2] << 16)
        for i in range(total)
    ]
    data_area = off_tbl + total * 3
    placements = []
    skipped_no_pos = 0
    # Caller iterates s4 in [1, total-a) -> a3 in [a+1, total).
    for s4 in range(1, total - a):
        a3 = a + s4
        if a3 >= total:
            break
        rec_off = data_area + offsets[a3]
        # Walker (asm at 0x8003A2B0): byte 0 = n_chars; advance by 1 + 2*n_chars
        # to reach the (tmd, flag, x, z) suffix.
        n_chars = man[rec_off]
        name_end = rec_off + 1 + 2 * n_chars
        s1 = name_end
        if s1 + 4 > len(man):
            break
        try:
            name = man[rec_off + 1:name_end].decode("shift_jis", errors="replace")
        except Exception:
            name = repr(man[rec_off + 1:name_end])
        tmd, flag, x_enc, z_enc = man[s1], man[s1 + 1], man[s1 + 2], man[s1 + 3]
        # (0x7F, 0x7F) = "no static position; spawn point set by field-VM script"
        script_positioned = (x_enc == 0x7F and z_enc == 0x7F)
        if script_positioned:
            skipped_no_pos += 1
        x = ((x_enc & 0x7F) << 7) + (0x80 if (x_enc & 0x80) else 0x40)
        z = ((z_enc & 0x7F) << 7) + (0x80 if (z_enc & 0x80) else 0x40)
        placements.append({
            "id": s4,
            "name": name,
            "tmd_slot": tmd,
            "flag": flag,
            "flags": f"0x{flag:02X}",  # legacy field name for site compat
            "list": flag_to_list(flag),
            "layer": ["player/system", "entities", "background", "reserve_3",
                      "reserve_4", "reserve_5", "extra"][flag_to_list(flag)],
            "pos": [x, 0, z],
            "x_enc": x_enc,
            "z_enc": z_enc,
            "script_positioned": script_positioned,
        })
    return {
        "a": a, "b": b, "c": c, "total": total,
        "placements": placements,
        "script_positioned_count": skipped_no_pos,
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--prot-dir", default="/tmp/legaia-extract/PROT",
                    help="Directory containing extracted PROT.DAT entries "
                         "(NNNN_<cdname>.BIN). Default: %(default)s")
    ap.add_argument("--lzs-bin", default="target/release/lzs-decode",
                    help="Path to the lzs-decode CLI binary.")
    ap.add_argument("--out", default="site/world-overview.json",
                    help="Output JSON path for the site page.")
    args = ap.parse_args()

    prot_dir = Path(args.prot_dir)
    lzs_bin = Path(args.lzs_bin)
    out = Path(args.out)
    if not lzs_bin.exists():
        sys.exit(
            f"lzs-decode CLI not found at {lzs_bin}.\n"
            f"Run `cargo build --release -p legaia-lzs` first."
        )

    payload = {}
    for base, key, label, cdname in KINGDOMS:
        matches = sorted(prot_dir.glob(f"{base:04d}_*.BIN"))
        if not matches:
            sys.exit(f"PROT entry {base:04d}_*.BIN missing under {prot_dir}.\n"
                     f"Run `target/release/legaia-extract <DISC> --out /tmp/legaia-extract` first.")
        prot_path = matches[0]
        man = extract_man(prot_path, lzs_bin)
        parsed = parse_placements(man)
        # Camera centroid uses only world-positioned actors (skip the
        # script-positioned 0x7F-sentinel records).
        real = [p for p in parsed["placements"] if not p["script_positioned"]]
        if real:
            xs = [p["pos"][0] for p in real]
            zs = [p["pos"][2] for p in real]
            cx, cz = sum(xs) // len(xs), sum(zs) // len(zs)
        else:
            cx, cz = 8000, 8000
        payload[key] = {
            "kingdom": key,
            "label": label,
            "cdname": cdname,
            "prot_base": base,
            "camera": {"x": cx, "z": cz, "azimuth": 0, "zoom": 1.0},
            "tmd_count": 119,
            "world_extent": [16320, 16320],
            "header": {"a": parsed["a"], "b": parsed["b"], "c": parsed["c"], "total": parsed["total"]},
            "placements": parsed["placements"],
            "world_placed_count": len(real),
            "script_positioned_count": parsed["script_positioned_count"],
        }
        print(f"{label:18s} (PROT {base}, {prot_path.name}): "
              f"{len(parsed['placements'])} total records, "
              f"{len(real)} world-placed, "
              f"{parsed['script_positioned_count']} script-positioned")

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, ensure_ascii=False, indent=2))
    print(f"\nWrote {out} ({out.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
