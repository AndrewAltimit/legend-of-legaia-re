#!/usr/bin/env python3
"""Scan a PSX RAM dump for textured-sprite primitives (GP0 0x64) that
draw inside a target framebuffer rect.

PSX libgpu primitive layout for a textured sprite (GP0 0x64..0x67):
    +0  u32  tag        (low 24 = next-ptr, high 8 = length-1 in words)
    +4  u32  cmd|rgb    (cmd byte 0x64..0x67, rgb in low 24)
    +8  s16  dst_x
    +10 s16  dst_y
    +12 u8   tex_u
    +13 u8   tex_v
    +14 u16  clut       (clut_y << 6 | clut_x>>4 in standard encoding)
    +16 s16  width
    +18 s16  height

The texture page is set by a preceding DR_TPAGE primitive and isn't
stored in the sprite primitive itself. CLUT is encoded as
`(clut_y << 6) | (clut_x >> 4)`; this script decodes it back to
absolute VRAM coords.

Pairs with `autorun_load_screen_dump.lua` and other probes that
capture `load_screen_ram.bin` (or any 2 MiB main-RAM snapshot). The
output identifies every sprite the engine queued in the target rect
along with its source u/v + CLUT - enough to cross-reference back
to the source TIM in `PROT.DAT` and pin tile geometry byte-equal to
retail.

Usage:
    scan_panel_prims.py <ram_dump.bin> [x0 y0 x1 y1]

Default rect is the Continue->Load screen panel (6, 4, 87, 33).
"""
import argparse
import struct
import sys
from pathlib import Path
from collections import defaultdict


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("ram", type=Path, help="2 MiB main-RAM dump")
    p.add_argument("--rect", nargs=4, type=int, default=[6, 4, 87, 33],
                   metavar=("X0", "Y0", "X1", "Y1"),
                   help="framebuffer rect to filter sprite dst against (inclusive)")
    p.add_argument("--cmd", default="0x64,0x65,0x66,0x67",
                   help="comma-separated GP0 sprite cmd bytes to match")
    args = p.parse_args()

    ram = args.ram.read_bytes()
    PX0, PY0, PX1, PY1 = args.rect
    cmds = {int(c, 0) for c in args.cmd.split(",")}

    candidates = []
    for off in range(0, len(ram) - 20, 4):
        cmd_byte = ram[off + 4 + 3]
        if cmd_byte not in cmds:
            continue
        dst_x, dst_y = struct.unpack_from("<hh", ram, off + 8)
        tex_u = ram[off + 12]
        tex_v = ram[off + 13]
        clut = struct.unpack_from("<H", ram, off + 14)[0]
        w, h = struct.unpack_from("<hh", ram, off + 16)
        if not (PX0 - 2 <= dst_x <= PX1 + 2):
            continue
        if not (PY0 - 2 <= dst_y <= PY1 + 2):
            continue
        if not (1 <= w <= 256 and 1 <= h <= 64):
            continue
        clut_x = (clut & 0x3F) * 16
        clut_y = (clut >> 6) & 0x1FF
        candidates.append((off, cmd_byte, dst_x, dst_y, tex_u, tex_v,
                           clut, clut_x, clut_y, w, h))

    print(f"Found {len(candidates)} textured-sprite primitives "
          f"in rect ({PX0},{PY0})..({PX1},{PY1}).\n")
    print(f"{'RAM_off':>10} {'cmd':>4} {'dst':>10} {'uv':>10}"
          f" {'CLUT(fb)':>11} {'w x h':>8}")
    for off, cb, dx, dy, u, v, _, cx, cy, w, h in candidates:
        print(f"  0x{off:08X} 0x{cb:02X} ({dx:3d},{dy:3d})  "
              f"({u:3d},{v:3d})  ({cx:3d},{cy:3d})  {w:3d}x{h:2d}")

    # Group by CLUT - distinct CLUTs typically = distinct source TIMs.
    by_clut = defaultdict(list)
    for c in candidates:
        by_clut[(c[7], c[8])].append((c[2], c[3], c[4], c[5], c[9], c[10]))

    print(f"\nUnique CLUTs in scan: {len(by_clut)}")
    for (cx, cy), draws in by_clut.items():
        unique_uv = {(u, v, w, h) for _, _, u, v, w, h in draws}
        print(f"\n  CLUT (fb_x={cx}, fb_y={cy}): {len(draws)} draws, "
              f"{len(unique_uv)} unique source tiles")
        for uv in sorted(unique_uv):
            print(f"    src ({uv[0]:3d},{uv[1]:3d})  size {uv[2]:3d}x{uv[3]:2d}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
