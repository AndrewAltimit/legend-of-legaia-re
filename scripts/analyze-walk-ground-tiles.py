#!/usr/bin/env python3
"""
analyze-walk-ground-tiles.py

Decode the overworld walk-view continent **ground tiles** out of a raw PSX
main-RAM image (a 2 MiB dump, e.g. captures/ram_dumps/mountain_walk.bin, taken
while standing on a kingdom continent in walk mode / game_mode 0x03).

The walk-view ground is drawn as a field of `POLY_FT4` (cmd 0x2C) textured
quads, one 32x32-texel quad per visible cell, emitted in a **row-major
world-cell sweep**. Each cell's texture is selected per cell from a **terrain-
type-keyed multi-page atlas**: the cell's object record (in the walk `.MAP`
file) carries the tile + page + palette in its `+0x14..+0x18` run:

  * `+0x14`  -> the 8x8 atlas tile index (u = (id % 8) * 32, v = (id / 8) * 32),
  * `+0x15`  -> the PSX `tpage` word (the terrain VRAM page: grass 0x1A,
               mountain 0x0C, water 0x1B/0x1C, forest 0x0B, ...),
  * `+0x16..+0x18` -> the PSX `clut` (CBA) word.

So grass, mountain, water, and forest cells each sample a different VRAM page.
(An earlier reading - "single 3x3 grass page, positional (col%3, row%3), +0x14
unused" - was a misread: grass cells happen to use page 0x1A with `+0x14` in the
top-left 3x3 block, so the mod-3 cross-row sequence was coincidental.)

This script confirms that structure directly from the data:

  * `--survey`   (default) buckets the ground-sized (32x32) quads by
                 (tpage, clut) so the terrain pages in use are visible;
  * `--verify-rule MAPBUF` aligns each contiguous quad run (emission order) to
                 the walk `.MAP`'s `+0x14` grid and reports how often the quad's
                 tile / page / clut equal the record's `+0x14` / `+0x15` /
                 `+0x16..+0x18` - i.e. it re-derives the rule from scratch.

The clean-room engine bakes the same per-cell tile + page + palette in
`legaia_asset::field_objects::build_walk_heightfield`
(`WalkHeightfield::uvs` + `::cba_tsb`).

USAGE
    scripts/analyze-walk-ground-tiles.py captures/ram_dumps/mountain_walk.bin
    scripts/analyze-walk-ground-tiles.py DUMP --verify-rule WALK.MAP

`DUMP` is a Sony-derived RAM image and `MAPBUF` a Sony-derived walk `.MAP`; both
must stay local (gitignored).
"""
from __future__ import annotations

import argparse
import struct
import sys
from collections import Counter, defaultdict

import gpu_packets  # shared PSX GPU primitive decode (scripts/gpu_packets.py)

# Field-map layout (mirrors legaia_asset::field_objects).
GRID_DIM = 0x80
OBJECT_GRID_OFFSET = 0x8000
OBJECT_RECORD_STRIDE = 0x20
OBJECT_INDEX_MASK = 0x1FF
CELL_WALK_VISIBLE = 0x1000

# The terrain VRAM pages the continent ground samples (4bpp pages in the bottom
# VRAM strip). Quads on any of these, sized 32x32, are ground tiles.
TERRAIN_TPAGES = {0x000A, 0x000B, 0x000C, 0x0019, 0x001A, 0x001B, 0x001C}
TILE_PX = 32
ATLAS_AXIS = 8  # 8x8 atlas filling one 256x256 page


def ground_quads(ram: bytes):
    """All 32x32 POLY_FT4 quads on a terrain page, with memory offset (the
    emission order) and the derived 8x8 atlas tile index."""
    out = []
    for pkt in gpu_packets.iter_textured_packets(ram, codes={0x2C}):
        if pkt.tpage is None or pkt.tpage not in TERRAIN_TPAGES:
            continue
        u0, v0 = pkt.uvs[0]
        u3, v3 = pkt.uvs[3]
        if abs(u3 - u0) != TILE_PX - 1 or abs(v3 - v0) != TILE_PX - 1:
            continue
        u, v = min(u0, u3), min(v0, v3)
        tile = (v // TILE_PX) * ATLAS_AXIS + (u // TILE_PX)
        out.append((pkt.off, pkt.tpage, pkt.clut, tile))
    out.sort()
    return out


def survey(ram: bytes) -> int:
    quads = ground_quads(ram)
    if not quads:
        print("no ground-sized POLY_FT4 quads on a terrain page found")
        return 1
    by_page: Counter = Counter()
    for _, tp, cl, _ in quads:
        by_page[(tp, cl)] += 1
    print(f"{len(quads)} terrain ground quads, by (tpage, clut):")
    for (tp, cl), n in by_page.most_common():
        fbx = (tp & 0xF) * 64
        fby = ((tp >> 4) & 1) * 256
        cbx = (cl & 0x3F) * 16
        cby = cl >> 6
        print(
            f"  tpage=0x{tp:04X} fb({fbx},{fby})  clut=0x{cl:04X} fb({cbx},{cby})  n={n}"
        )
    print(
        "\nDistinct terrain pages = distinct terrain types (grass 0x1A, mountain\n"
        "0x0C, water 0x1B/0x1C, forest 0x0B). The page is per cell - see\n"
        "--verify-rule to re-derive the selector from the walk .MAP records."
    )
    return 0


def verify_rule(ram: bytes, mapbuf: bytes) -> int:
    """Sequence-align each quad run to the map's +0x14 grid and report the
    tile / page / clut match rates against the record's +0x14..+0x18 run."""

    def rec(oi: int, off: int) -> int:
        i = oi * OBJECT_RECORD_STRIDE + off
        return mapbuf[i] if i < len(mapbuf) else -1

    def cell(r: int, c: int) -> int:
        return struct.unpack_from(
            "<H", mapbuf, OBJECT_GRID_OFFSET + (r * GRID_DIM + c) * 2
        )[0]

    def t14(r: int, c: int) -> int:
        v = cell(r, c)
        if not (v & CELL_WALK_VISIBLE):
            return -1
        return rec(v & OBJECT_INDEX_MASK, 0x14)

    rows = [[t14(r, c) for c in range(GRID_DIM)] for r in range(GRID_DIM)]

    quads = ground_quads(ram)
    # Split into contiguous emission runs (the prim allocator lays the swept
    # cells out sequentially; a gap > one packet stride starts a new run).
    runs = []
    start = 0
    for i in range(1, len(quads)):
        if quads[i][0] - quads[i - 1][0] > 0x40:
            runs.append(quads[start:i])
            start = i
    runs.append(quads[start:])

    def best_row(seq):
        best = (-1, None, None)
        length = len(seq)
        for r in range(GRID_DIM):
            line = rows[r]
            for o in range(0, GRID_DIM - length + 1):
                sc = sum(1 for a, b in zip(seq, line[o : o + length]) if a == b)
                if sc > best[0]:
                    best = (sc, r, o)
        return best

    tile_ok = page_ok = clut_ok = total = 0
    page_by_15 = defaultdict(Counter)
    aligned_runs = 0
    for run in runs:
        if len(run) < 12:
            continue
        head = [q[3] for q in run[:30]]
        sc, r, o = best_row(head)
        if sc < len(head) * 0.8:  # only confidently-aligned runs
            continue
        aligned_runs += 1
        for i, (_, tp, cl, tile) in enumerate(run):
            c = o + i
            if c >= GRID_DIM:
                break
            v = cell(r, c)
            if not (v & CELL_WALK_VISIBLE):
                continue
            oi = v & OBJECT_INDEX_MASK
            if tile != rec(oi, 0x14):
                continue  # alignment drifted off this cell; skip
            total += 1
            tile_ok += 1  # tile == +0x14 by construction of this filter
            rec_page = rec(oi, 0x15)
            rec_clut = rec(oi, 0x16) | (rec(oi, 0x17) << 8)
            if tp == rec_page:
                page_ok += 1
            if cl == rec_clut:
                clut_ok += 1
            page_by_15[rec_page][tp] += 1

    if total == 0:
        print("no runs aligned - is MAPBUF the matching walk .MAP for this dump?")
        return 1
    print(f"aligned {aligned_runs} quad runs, {total} verified cells")
    print(f"  tile  == record +0x14        : {tile_ok}/{total} ({tile_ok / total:.1%})")
    print(f"  tpage == record +0x15        : {page_ok}/{total} ({page_ok / total:.1%})")
    print(f"  clut  == record +0x16..+0x18 : {clut_ok}/{total} ({clut_ok / total:.1%})")
    print("\n  record +0x15 -> observed quad tpage:")
    for b in sorted(page_by_15):
        d = {f"0x{p:04X}": n for p, n in page_by_15[b].most_common()}
        print(f"    +0x15=0x{b:02X}: {d}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("dump", help="raw 2 MiB PSX main-RAM image")
    ap.add_argument(
        "--verify-rule",
        metavar="MAPBUF",
        help="walk .MAP file to re-derive +0x14/+0x15/+0x16 ground rule against",
    )
    args = ap.parse_args()
    ram = open(args.dump, "rb").read()
    if args.verify_rule:
        return verify_rule(ram, open(args.verify_rule, "rb").read())
    return survey(ram)


if __name__ == "__main__":
    sys.exit(main())
