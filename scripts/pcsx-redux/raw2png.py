#!/usr/bin/env python3
"""Convert probe .raw+.raw.meta screenshots to PNG (BGR555/RGB24)."""
import sys, pathlib
from PIL import Image

def conv(raw_path):
    p = pathlib.Path(raw_path)
    meta = {}
    for line in (p.with_suffix(p.suffix + ".meta")).read_text().splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            meta[k.strip()] = int(v.strip())
    w, h, bpp = meta["width"], meta["height"], meta["bpp"]
    raw = p.read_bytes()
    if bpp == 16:
        px = bytearray(w * h * 3)
        for i in range(w * h):
            half = raw[i*2] | (raw[i*2+1] << 8)
            r5, g5, b5 = half & 0x1F, (half >> 5) & 0x1F, (half >> 10) & 0x1F
            px[i*3]   = (r5 << 3) | (r5 >> 2)
            px[i*3+1] = (g5 << 3) | (g5 >> 2)
            px[i*3+2] = (b5 << 3) | (b5 >> 2)
        img = Image.frombytes("RGB", (w, h), bytes(px))
    else:
        img = Image.frombytes("RGB", (w, h), raw[:w*h*3])
    out = p.with_suffix(".png")
    img.save(out)
    print(out)

for a in sys.argv[1:]:
    conv(a)
