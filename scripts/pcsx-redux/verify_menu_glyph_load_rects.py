#!/usr/bin/env python3
"""Falsify the (192,32)/(240,64)/(16,64)/(64,64) Load-glyph pin in the
menu-glyph atlas at `PROT.DAT[0x11218]`.

The earlier pin in docs/subsystems/save-screen.md claimed the
load-screen "Load" title sampled the menu-glyph atlas at the four
documented rects with CLUT row 13. This script reads PROT.DAT
directly, parses the menu-glyph TIM at offset 0x11218, and dumps the
raw 4bpp pixel indices at each documented rect. All four rects come
back as zero (transparent) indices - the menu-glyph atlas does not
carry the Load title glyphs at those positions in any CLUT row.

The correct source is the dialog font at runtime VRAM tpage 14
(VRAM 896, 0), with CLUT at VRAM (208, 510). See
[[project-load-screen-title-glyph-source-pinned]] in agent memory
and the engine-render `SAVE_SELECT_TITLE_POS` / `SAVE_SELECT_TITLE_COLOR`
constants for the live pin.

Run:
    python3 scripts/pcsx-redux/verify_menu_glyph_load_rects.py extracted/PROT.DAT
"""
import struct
import sys
from pathlib import Path


PROT_DAT_OFFSET = 0x11218


def parse_tim(buf: bytes, off: int):
    p = off + 8
    (clut_size,) = struct.unpack_from('<I', buf, p)
    clut = buf[p + 12: p + clut_size]
    p += clut_size
    (pix_size,) = struct.unpack_from('<I', buf, p)
    pix_w, pix_h = struct.unpack_from('<HH', buf, p + 8)
    pix = buf[p + 12: p + pix_size]
    return clut, pix, pix_w * 4, pix_h


def index_at(pix: bytes, w: int, x: int, y: int) -> int:
    b = pix[y * (w // 2) + x // 2]
    return (b & 0xF) if (x & 1) == 0 else (b >> 4) & 0xF


def main() -> int:
    prot = Path(sys.argv[1]).read_bytes()
    _clut, pix, w, _h = parse_tim(prot, PROT_DAT_OFFSET)
    rects = [
        ('L', 192, 32, 14, 15),
        ('o', 240, 64, 14, 15),
        ('a', 16, 64, 14, 15),
        ('d', 64, 64, 14, 15),
    ]
    failures = 0
    for name, x, y, rw, rh in rects:
        opaque = 0
        for yy in range(y, y + rh):
            for xx in range(x, x + rw):
                if index_at(pix, w, xx, yy) != 0:
                    opaque += 1
        verdict = 'EMPTY' if opaque == 0 else f'{opaque} opaque'
        print(f"{name} @ ({x},{y},{rw},{rh}): {verdict}")
        if opaque != 0:
            failures += 1
    if failures > 0:
        print("\nUNEXPECTED: rects carry opaque pixels - the menu-glyph "
              "atlas may have changed, or you're pointing at a different "
              "PROT.DAT region.")
        return 1
    print("\nFalsified: menu-glyph atlas does not carry the Load title "
          "glyphs at the documented rects under any CLUT row.")
    print("True source: dialog font @ VRAM tpage 14 (VRAM 896, 0), "
          "CLUT @ VRAM (208, 510). See "
          "legaia_engine_render::SAVE_SELECT_TITLE_{POS,COLOR}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
