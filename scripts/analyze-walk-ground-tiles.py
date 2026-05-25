#!/usr/bin/env python3
"""
analyze-walk-ground-tiles.py

Decode the overworld walk-view continent **ground tiles** out of a raw PSX
main-RAM image (a 2 MiB dump, e.g. captures/ram_dumps/drake_walk.bin, taken
while standing on a kingdom continent in walk mode / game_mode 0x03).

The walk-view ground is drawn as a field of `POLY_FT4` (cmd 0x2C) textured
quads, one per visible cell in a window around the player. They all sample a
single VRAM page through one CLUT, and each quad is a 32x32-texel rect taken
from a small 3x3 atlas of ground tiles. The tile is selected **positionally**
(`col % 3`, `row % 3`) - a detail-tiling trick that hides the repetition of one
ground texture - which is why no per-cell record field (the `+0x14` byte) feeds
it. This script confirms that structure directly from the prim pool:

  * scans main RAM for POLY_FT4 packets whose CLUT/TPAGE match the ground page,
  * reports the distinct tile UV origins (the atlas grid) and per-tile texel
    size, and
  * checks the positional mod-3 cycle by correlating each quad's atlas column
    with its on-screen X (and row with screen Y).

The clean-room engine bakes the same `(col%3, row%3)` UVs in
`legaia_asset::field_objects::build_walk_heightfield`
(constants `GROUND_ATLAS_TPAGE` / `GROUND_ATLAS_CLUT` / `_TILE_PX` / `_AXIS`).

USAGE
    scripts/analyze-walk-ground-tiles.py captures/ram_dumps/drake_walk.bin
    scripts/analyze-walk-ground-tiles.py DUMP --clut 0x7C40 --tpage 0x001A

The dump is a Sony-derived RAM image and must stay local (gitignored).
"""
from __future__ import annotations

import argparse
import sys
from collections import Counter

import gpu_packets  # shared PSX GPU primitive decode (scripts/gpu_packets.py)

# Default ground-page identifiers, pinned from a Drake (map01) walk image.
DEFAULT_CLUT = 0x7C40  # CBA word -> VRAM fb (0, 497)
DEFAULT_TPAGE = 0x001A  # 4bpp page -> VRAM fb (640, 256)

# The ground is drawn as POLY_FT4 (flat-textured quad) packets; the shared
# library knows the libgpu packet layout (tag + GP0 words, clut in uv0's high
# half, tpage in uv1's high half). See scripts/gpu_packets.py.


def decode(ram: bytes, clut: int, tpage: int):
    quads = []
    for pkt in gpu_packets.iter_textured_packets(
        ram, clut=clut, tpage=tpage, codes={0x2C}
    ):
        u0, v0 = pkt.uvs[0]
        u3, v3 = pkt.uvs[3]
        sx, sy = pkt.xys[0]
        quads.append(
            dict(
                off=pkt.off,
                uv0=(u0, v0),
                span=(abs(u3 - u0), abs(v3 - v0)),
                sx=sx,
                sy=sy,
            )
        )
    return quads


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("dump", help="raw 2 MiB PSX main-RAM image")
    ap.add_argument("--clut", type=lambda s: int(s, 0), default=DEFAULT_CLUT)
    ap.add_argument("--tpage", type=lambda s: int(s, 0), default=DEFAULT_TPAGE)
    ap.add_argument(
        "--rows",
        type=int,
        default=0,
        help="print the per-screen-row tile-column sequence for N rows",
    )
    args = ap.parse_args()

    ram = open(args.dump, "rb").read()
    quads = decode(ram, args.clut, args.tpage)
    if not quads:
        print(
            f"no POLY_FT4 ground quads (clut=0x{args.clut:04X} "
            f"tpage=0x{args.tpage:04X}) found in {args.dump}"
        )
        return 1

    page_x = (args.tpage & 0xF) * 64
    page_y = ((args.tpage >> 4) & 1) * 256
    cba_x = (args.clut & 0x3F) * 16
    cba_y = args.clut >> 6
    print(f"{len(quads)} ground quads (clut=0x{args.clut:04X} tpage=0x{args.tpage:04X})")
    print(f"  VRAM page fb ({page_x}, {page_y})   CLUT fb ({cba_x}, {cba_y})")

    spans = Counter(q["span"] for q in quads)
    print(f"  tile texel size(s): {spans.most_common()}")

    origins = Counter(q["uv0"] for q in quads)
    us = sorted({u for u, _ in origins})
    vs = sorted({v for _, v in origins})
    print(f"  distinct tile UV origins: {len(origins)} (u in {us}, v in {vs})")
    for uv, n in sorted(origins.items()):
        print(f"    uv0={uv}: {n}")

    # Positional check: does the atlas column track screen X mod 3? Pick the
    # tile size to derive the column index, then bin quads by rounded screen X
    # and see whether the column cycles 0,1,2 with adjacent screen columns.
    tile_px = spans.most_common(1)[0][0][0] + 1  # span is size-1
    by_sx = {}
    for q in quads:
        col_idx = q["uv0"][0] // tile_px
        by_sx.setdefault(round(q["sx"] / max(tile_px // 3, 1)), Counter())[col_idx] += 1
    # Report the dominant column index per screen-X bucket, in order.
    seq = [c.most_common(1)[0][0] for _, c in sorted(by_sx.items())]
    cyc = "".join(str(s) for s in seq[:60])
    print(f"  atlas-column vs screen-X (first 60 buckets): {cyc}")
    print(
        "  note: screen axes are camera-rotated, so this cross-row sequence is\n"
        "  only approximately cyclic; the clean 0,1,2 cycle is per screen-row\n"
        "  (constant Y) - inspect with --rows for a single-row slice"
    )
    if args.rows:
        print("\n  per-screen-row tile columns (sy: u-origin sequence):")
        rows = {}
        for q in quads:
            rows.setdefault(q["sy"], []).append((q["sx"], q["uv0"][0] // tile_px))
        for sy in sorted(rows)[: args.rows]:
            cols = [c for _, c in sorted(rows[sy])]
            print(f"    sy={sy:4d}: {''.join(str(c) for c in cols)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
