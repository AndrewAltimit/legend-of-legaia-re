#!/usr/bin/env python3
"""Measure save-UI messagebox panel rects from a captured framebuffer.

The save UI's panels are drawn with a distinctive gold/amber border on a
dark blue fill. This finds those borders and reports each panel's rect, so a
rect can be *measured* off retail pixels instead of inferred from a sibling
panel's size.

Pairs with `autorun_confirm_dialog_dump.lua` (and works on any save-UI
framebuffer capture, e.g. the "Now checking" dump). Feed it the decoded PNG
from `decode_load_screen.py`, or the raw+meta pair directly.

Method: classify every pixel as "gold border ink" by hue (red and green both
high, blue clearly lower - the amber border), then find maximal horizontal
runs of gold. A panel's top and bottom edges are long runs at the same x
span; the rect is their bounding box. Runs shorter than --min-run are
ignored so glyph antialiasing and the small badge sprites don't register as
panel edges.

Usage:
    scan_panel_rects.py <capture_dir> --stem confirm_dialog_fb
    scan_panel_rects.py <capture_dir> --stem now_checking_fb --min-run 40
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


def parse_meta(meta_path: Path) -> dict[str, int]:
    out: dict[str, int] = {}
    for raw in meta_path.read_text().splitlines():
        line = raw.strip()
        if not line or "=" not in line:
            continue
        k, v = line.split("=", 1)
        out[k.strip()] = int(v.strip())
    return out


def load_rgb(capture_dir: Path, stem: str) -> tuple[list[list[tuple[int, int, int]]], int, int]:
    """Load the raw framebuffer as a row-major RGB grid."""
    meta = parse_meta(capture_dir / f"{stem}.meta")
    w, h, bpp = meta["width"], meta["height"], meta["bpp"]
    raw = (capture_dir / f"{stem}.raw").read_bytes()
    rows: list[list[tuple[int, int, int]]] = []
    for y in range(h):
        row: list[tuple[int, int, int]] = []
        for x in range(w):
            if bpp == 16:
                i = (y * w + x) * 2
                half = raw[i] | (raw[i + 1] << 8)
                r5, g5, b5 = half & 0x1F, (half >> 5) & 0x1F, (half >> 10) & 0x1F
                row.append((
                    (r5 << 3) | (r5 >> 2),
                    (g5 << 3) | (g5 >> 2),
                    (b5 << 3) | (b5 >> 2),
                ))
            else:
                i = (y * w + x) * 3
                row.append((raw[i], raw[i + 1], raw[i + 2]))
        rows.append(row)
    return rows, w, h


def is_gold(px: tuple[int, int, int]) -> bool:
    """The amber panel border: red high, green mid-high, blue clearly lower."""
    r, g, b = px
    return r >= 120 and g >= 70 and b + 40 <= r and r > b and g > b


def gold_runs(rows, w: int, h: int, min_run: int) -> dict[int, list[tuple[int, int]]]:
    """Per row, the maximal gold runs at least min_run px wide."""
    out: dict[int, list[tuple[int, int]]] = {}
    for y in range(h):
        runs = []
        x = 0
        while x < w:
            if is_gold(rows[y][x]):
                start = x
                while x < w and is_gold(rows[y][x]):
                    x += 1
                if x - start >= min_run:
                    runs.append((start, x - 1))
            else:
                x += 1
        if runs:
            out[y] = runs
    return out


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("capture_dir", type=Path)
    p.add_argument("--stem", default="confirm_dialog_fb")
    p.add_argument("--min-run", type=int, default=30,
                   help="ignore gold runs narrower than this (default 30)")
    args = p.parse_args()

    rows, w, h = load_rgb(args.capture_dir, args.stem)
    runs = gold_runs(rows, w, h, args.min_run)
    if not runs:
        print("no gold runs found; is this a save-UI capture?", file=sys.stderr)
        return 1

    print(f"{args.stem}: {w}x{h}")
    print("\nhorizontal gold runs (candidate panel edges):")
    for y in sorted(runs):
        spans = ", ".join(f"x={a}..{b} (w={b - a + 1})" for a, b in runs[y])
        print(f"  y={y:3d}  {spans}")

    # Pair each long run with a matching run lower down at the same span:
    # that pair is a panel's top and bottom edge.
    print("\npanels (top edge paired with an equal-span bottom edge):")
    used: set[tuple[int, int, int]] = set()
    for y in sorted(runs):
        for a, b in runs[y]:
            if (y, a, b) in used:
                continue
            for y2 in sorted(runs):
                if y2 <= y:
                    continue
                if any(a == a2 and b == b2 for a2, b2 in runs[y2]):
                    used.add((y2, a, b))
                    print(f"  rect: pos=({a}, {y})  size=({b - a + 1}, {y2 - y + 1})"
                          f"   [rows {y}..{y2}, cols {a}..{b}]")
                    break
    return 0


if __name__ == "__main__":
    sys.exit(main())
