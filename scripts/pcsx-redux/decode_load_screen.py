#!/usr/bin/env python3
"""Decode autorun_load_screen_dump.lua output into a PNG framebuffer.

Pairs with `scripts/pcsx-redux/autorun_load_screen_dump.lua`: that
probe captures the rendered load-screen framebuffer at PCSX-Redux
sstate9 as raw bytes (BGR555 or RGB24 depending on the GPU's 16/24
bpp mode at the time). This script converts the raw bytes + meta
file pair into a `.png` we can inspect / measure / cross-reference
against the extracted PROT TIM corpus.

The PNG is the byte-exact retail framebuffer at the load-screen
state. Pixel coordinates in this PNG match PSX framebuffer
coordinates 1:1 (i.e. the panel sits at PSX (x, y) etc.), so
sprite-rect dst positions can be measured directly.

Usage:
    decode_load_screen.py <capture_dir>
       reads <dir>/load_screen_fb.raw + <dir>/load_screen_fb.meta
       writes <dir>/load_screen_fb.png

Why a separate script instead of doing it in-Lua: PCSX-Redux's
Lua runtime ships without a PNG encoder. Doing the conversion in
stdlib Python (PIL) on the host keeps the probe Lua minimal.
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path


def parse_meta(meta_path: Path) -> dict[str, int]:
    """Parse the `key=value` meta file the probe writes."""
    out: dict[str, int] = {}
    for raw in meta_path.read_text().splitlines():
        line = raw.strip()
        if not line or "=" not in line:
            continue
        k, v = line.split("=", 1)
        out[k.strip()] = int(v.strip())
    return out


def bgr555_to_rgb888(half: int) -> tuple[int, int, int]:
    """Decode a 15-bit BGR555 word (PSX VRAM native) to RGB888."""
    r5 = half & 0x1F
    g5 = (half >> 5) & 0x1F
    b5 = (half >> 10) & 0x1F
    # PSX expands 5-bit to 8-bit by replicating the high bits.
    r = (r5 << 3) | (r5 >> 2)
    g = (g5 << 3) | (g5 >> 2)
    b = (b5 << 3) | (b5 >> 2)
    return r, g, b


def decode(raw: bytes, w: int, h: int, bpp: int) -> bytes:
    """Convert PSX framebuffer bytes -> packed RGB888 row-major bytes."""
    if bpp == 16:
        expected = w * h * 2
        if len(raw) != expected:
            raise SystemExit(
                f"BGR555 raw size {len(raw)} != {w}*{h}*2 = {expected}"
            )
        pixels = bytearray(w * h * 3)
        for i in range(w * h):
            half = raw[i * 2] | (raw[i * 2 + 1] << 8)
            r, g, b = bgr555_to_rgb888(half)
            pixels[i * 3] = r
            pixels[i * 3 + 1] = g
            pixels[i * 3 + 2] = b
        return bytes(pixels)
    if bpp == 24:
        expected = w * h * 3
        if len(raw) != expected:
            raise SystemExit(
                f"RGB24 raw size {len(raw)} != {w}*{h}*3 = {expected}"
            )
        return raw
    raise SystemExit(f"unsupported bpp={bpp}; expected 16 or 24")


def write_png(path: Path, rgb_bytes: bytes, w: int, h: int) -> None:
    """Write a minimal PNG without dragging in PIL.

    Uses stdlib zlib for the deflate stream + manual chunk emission.
    Keeps the script dependency-free.
    """
    import struct
    import zlib

    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)  # 8-bit RGB
    # IDAT: filter byte 0 per row, then raw rgb pixels.
    rows = bytearray()
    stride = w * 3
    for y in range(h):
        rows.append(0)
        rows.extend(rgb_bytes[y * stride : (y + 1) * stride])
    idat = zlib.compress(bytes(rows), 9)
    path.write_bytes(sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b""))


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "capture_dir",
        type=Path,
        help="Directory containing load_screen_fb.raw + load_screen_fb.meta",
    )
    p.add_argument(
        "-o",
        "--out",
        type=Path,
        default=None,
        help="Output PNG path (default: <capture_dir>/load_screen_fb.png)",
    )
    args = p.parse_args()

    raw_path = args.capture_dir / "load_screen_fb.raw"
    meta_path = args.capture_dir / "load_screen_fb.meta"
    out_path = args.out or (args.capture_dir / "load_screen_fb.png")
    if not raw_path.exists():
        raise SystemExit(f"missing {raw_path}")
    if not meta_path.exists():
        raise SystemExit(f"missing {meta_path}")

    meta = parse_meta(meta_path)
    w, h, bpp = meta["width"], meta["height"], meta["bpp"]
    raw = raw_path.read_bytes()
    rgb = decode(raw, w, h, bpp)
    write_png(out_path, rgb, w, h)
    print(f"wrote {out_path} ({w}x{h} @ {bpp}bpp)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
