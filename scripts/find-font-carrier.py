#!/usr/bin/env python3
"""Find the on-disc PROT entry that carries the dialog-font glyph bitmap.

Reads the raw 4bpp VRAM bytes the font extractor wrote to
`extracted/font/dialog_font_vram_4bpp.bin` and searches every PROT entry
for a long enough match. The font tile-page is 32 KB; a 64-byte slice
from the middle is enough to be unique against random bytes but small
enough to survive any minor permutations.

Run from the repo root:
    python3 scripts/find-font-carrier.py
"""
from __future__ import annotations
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXTRACTED = ROOT / "extracted"
FONT_BIN = EXTRACTED / "font" / "dialog_font_vram_4bpp.bin"
PROT_DIR = EXTRACTED / "PROT"


def main() -> int:
    if not FONT_BIN.exists():
        sys.exit(f"missing {FONT_BIN} - run `font-extract` first")
    if not PROT_DIR.exists():
        sys.exit(f"missing {PROT_DIR} - run `legaia-extract` first")

    font_bytes = FONT_BIN.read_bytes()
    # Pick a slice that's deep enough into the font to skip the leading
    # all-zeros region (space + control glyphs) and the all-zeros tail.
    candidates = [
        (0x600, 64),    # near start of letter 'A' (row 0, col 1)
        (0x1000, 64),   # mid-page
        (0x2000, 64),   # roughly halfway
        (0x4000, 64),   # later page
    ]
    needles = []
    for off, n in candidates:
        slice_bytes = font_bytes[off:off + n]
        if any(slice_bytes):
            needles.append((off, slice_bytes))

    if not needles:
        sys.exit("font bytes are all zero - did extraction succeed?")

    print(f"searching {len(list(PROT_DIR.glob('*.BIN')))} PROT entries with {len(needles)} probes...")
    hits: dict[str, list[tuple[int, int]]] = {}
    for path in sorted(PROT_DIR.glob("*.BIN")):
        data = path.read_bytes()
        for needle_off, needle in needles:
            idx = data.find(needle)
            if idx >= 0:
                hits.setdefault(path.name, []).append((needle_off, idx))

    if not hits:
        print("no PROT entry contains a font slice - the font lives behind LZS or in a per-frame upload that we haven't traced yet")
        print("see docs/formats/dialog-font.md for the unblock paths")
        return 1

    print(f"\n{len(hits)} PROT entries match at least one probe:")
    for name, matches in sorted(hits.items()):
        first = matches[0]
        print(f"  {name}: {len(matches)} probe(s); first at vram_off=0x{first[0]:X} prot_off=0x{first[1]:X}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
