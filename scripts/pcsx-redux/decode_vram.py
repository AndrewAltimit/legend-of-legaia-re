#!/usr/bin/env python3
"""Decode a 1 MiB raw VRAM blob (1024x512 BGR555) to a PNG.

Pairs with `extract_vram_from_sstate.py`: that pulls the 1 MiB GPU.vram
section out of a PCSX-Redux save state; this renders it as a PNG so
texture pages, CLUT rows, and double-buffered framebuffers are
visible at a glance.

Usage:
    decode_vram.py <vram.bin> <out.png>

The PNG is 1024x512 BGR555 -> RGB888 (5-bit replicated to 8-bit per
channel, matching PSX hardware). Pixel coordinates map 1:1 to PSX
VRAM (fb_x, fb_y), so a CLUT at row 511 in the PNG sits at fb_y=511.

Stdlib-only (no Pillow) - uses zlib + manual PNG chunks.
"""
import struct
import sys
import zlib
from pathlib import Path


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit("usage: decode_vram.py <vram.bin> <out.png>")
    vram = Path(sys.argv[1]).read_bytes()
    out_path = Path(sys.argv[2])
    W, H = 1024, 512
    if len(vram) != W * H * 2:
        raise SystemExit(f"got {len(vram)} bytes, expected {W * H * 2}")

    px = bytearray(W * H * 3)
    for i in range(W * H):
        half = vram[i * 2] | (vram[i * 2 + 1] << 8)
        r5 = half & 0x1F
        g5 = (half >> 5) & 0x1F
        b5 = (half >> 10) & 0x1F
        px[i * 3] = (r5 << 3) | (r5 >> 2)
        px[i * 3 + 1] = (g5 << 3) | (g5 >> 2)
        px[i * 3 + 2] = (b5 << 3) | (b5 >> 2)

    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0)
    rows = bytearray()
    stride = W * 3
    for y in range(H):
        rows.append(0)
        rows.extend(px[y * stride : (y + 1) * stride])
    idat = zlib.compress(bytes(rows), 9)
    out_path.write_bytes(sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b""))
    print(f"wrote {out_path} ({W}x{H})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
