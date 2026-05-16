#!/usr/bin/env python3
"""Decode a PCSX-Redux `takeScreenShot()` output to a PNG.

The autorun_countdown_trigger.lua probe writes three files:
    <out>.bin           - 2 MiB main RAM dump (not used here)
    <out>.screen        - raw framebuffer pixels
    <out>.screen.meta   - text key=value: width, height, bpp (16 or 24)

For 16 bpp the pixels are PSX BGR555 + STP (0bMrrrrrgggggbbbbb little-endian).
For 24 bpp the pixels are BGR888 byte-packed (one pixel = 3 bytes B, G, R).

Usage:
    scripts/decode_pcsx_screen.py captures/boot_walk/title_live.bin.screen \\
        -o captures/boot_walk/title_live.png

Requires Pillow.
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path


def parse_meta(meta_path: Path) -> dict[str, int]:
    out: dict[str, int] = {}
    for line in meta_path.read_text().splitlines():
        if "=" not in line:
            continue
        k, v = line.split("=", 1)
        out[k.strip()] = int(v.strip())
    return out


def decode_bgr555(data: bytes, w: int, h: int) -> bytes:
    """Decode a w*h*2 BGR555 little-endian buffer to RGB888 bytes."""
    if len(data) < w * h * 2:
        raise ValueError(f"bgr555: need {w*h*2} bytes, got {len(data)}")
    rgb = bytearray(w * h * 3)
    j = 0
    for i in range(w * h):
        word = data[i * 2] | (data[i * 2 + 1] << 8)
        r5 = word & 0x1F
        g5 = (word >> 5) & 0x1F
        b5 = (word >> 10) & 0x1F
        rgb[j + 0] = (r5 << 3) | (r5 >> 2)
        rgb[j + 1] = (g5 << 3) | (g5 >> 2)
        rgb[j + 2] = (b5 << 3) | (b5 >> 2)
        j += 3
    return bytes(rgb)


def decode_bgr888(data: bytes, w: int, h: int) -> bytes:
    """Decode a w*h*3 BGR888 buffer to RGB888 bytes (swap B and R)."""
    if len(data) < w * h * 3:
        raise ValueError(f"bgr888: need {w*h*3} bytes, got {len(data)}")
    rgb = bytearray(w * h * 3)
    for i in range(w * h):
        rgb[i * 3 + 0] = data[i * 3 + 2]  # R from byte 2
        rgb[i * 3 + 1] = data[i * 3 + 1]  # G from byte 1
        rgb[i * 3 + 2] = data[i * 3 + 0]  # B from byte 0
    return bytes(rgb)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("screen_path", type=Path,
                    help="Path to the .screen file (sidecar .meta must exist)")
    ap.add_argument("-o", "--out", type=Path, default=None,
                    help="PNG output path (default: <screen_path>.png)")
    args = ap.parse_args()

    screen_path: Path = args.screen_path
    meta_path = Path(str(screen_path) + ".meta")
    if not screen_path.exists():
        print(f"missing: {screen_path}", file=sys.stderr)
        return 1
    if not meta_path.exists():
        print(f"missing: {meta_path}", file=sys.stderr)
        return 1

    meta = parse_meta(meta_path)
    w = meta.get("width")
    h = meta.get("height")
    bpp = meta.get("bpp")
    if w is None or h is None or bpp is None:
        print(f"meta missing width/height/bpp: {meta}", file=sys.stderr)
        return 1

    data = screen_path.read_bytes()
    print(f"input: {screen_path} ({len(data)} bytes, {w}x{h} bpp={bpp})")

    if bpp == 16:
        rgb = decode_bgr555(data, w, h)
    elif bpp == 24:
        rgb = decode_bgr888(data, w, h)
    else:
        print(f"unsupported bpp: {bpp}", file=sys.stderr)
        return 1

    try:
        from PIL import Image  # noqa: WPS433
    except ImportError:
        print("Pillow not installed; writing raw RGB888 instead.", file=sys.stderr)
        out_path = args.out or screen_path.with_suffix(".rgb")
        out_path.write_bytes(rgb)
        print(f"wrote: {out_path} ({len(rgb)} bytes RGB888 raw)")
        return 0

    img = Image.frombytes("RGB", (w, h), rgb)
    out_path = args.out or screen_path.with_suffix(".png")
    img.save(out_path)
    print(f"wrote: {out_path} ({w}x{h} PNG)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
