#!/usr/bin/env python3
"""ot-raster.py - rasterise one of retail's battle ordering tables into a PNG.

`display-list.py` says which packets a frame links; this draws them. It walks
one of the two double-buffer records at `0x8007BF30` (`0x74` bytes each, the
bucket array at `+0x70`, 4096 buckets walked from the top) in chain order -
retail's painter's order, no depth buffer - and rasterises every polygon and
sprite packet against the state's own VRAM, at the GPU's 5-bit depth:

- textures read from VRAM (4 / 8 / 15 bpp, CLUT from the packet), texel `0`
  transparent, the `(t5 * c8) >> 7` texture blend unless the raw bit is set;
- flat colours truncated to 5 bits, Gouraud colours interpolated;
- the four ABR modes, on every pixel of an untextured semi prim and on the
  STP texels of a textured one;
- quads split `(v0, v1, v2)` + `(v1, v2, v3)`, UVs affine, prims wider than
  1023 or taller than 511 rejected as the GPU rejects them.

No dither and no mask bit; the canvas is the 320x224 draw area at offset
`(0, 0)`. The point is a second renderer that shares no code with the engine:
an engine frame seated on the inputs a table was built from should match its
raster exactly, so a residual against the **displayed** frame that survives
this check is not a rasterisation question.

Inputs are a raw main-RAM image and the raw 1 MiB VRAM blob:

    mednafen-state extract --start 0x80000000 --end 0x80200000 --out ram.bin STATE
    mednafen-state vram-dump --out vram.png --out-bin vram.bin STATE
    scripts/mednafen/ot-raster.py ram.bin vram.bin --record 0 --out ot0.png

`--record` picks the double-buffer record (`gp+0x434`, `0x8007B74C`, holds the
current one); `--buckets LO:HI` and `--clut 0x7900,...` / `--skip-clut` narrow
the draw to one body or layer; `--list` prints every drawn packet. Requires
numpy and Pillow. The output is a picture of retail's frame: keep it out of
the repository.
"""

from __future__ import annotations

import argparse
import struct
import sys

import numpy as np
from PIL import Image

KSEG0 = 0x8000_0000
DOUBLE_BUFFER = 0x8007_BF30
DOUBLE_BUFFER_STRIDE = 0x74
DOUBLE_BUFFER_OT = 0x70
OT_BUCKETS = 4096
W, H = 320, 224


class Raster:
    def __init__(self, ram: bytes, vram: np.ndarray):
        self.ram = ram
        self.vram = vram
        self.fb = np.zeros((H, W, 3), dtype=np.int32)
        self.tpage = 0

    def u32(self, a: int) -> int:
        return struct.unpack_from("<I", self.ram, (a - KSEG0) & 0x1F_FFFF)[0]

    def texel(self, tpage, clut, u, v):
        tx = (tpage & 0xF) * 64
        ty = ((tpage >> 4) & 1) * 256
        depth = (tpage >> 7) & 3
        u = u & 0xFF
        v = v & 0xFF
        cx = (clut & 0x3F) * 16
        cy = (clut >> 6) & 0x1FF
        if depth == 0:
            w = self.vram[(ty + v) & 511, (tx + (u >> 2)) & 1023]
            px = self.vram[cy, (cx + ((w >> ((u & 3) * 4)) & 0xF)) & 1023]
        elif depth == 1:
            w = self.vram[(ty + v) & 511, (tx + (u >> 1)) & 1023]
            px = self.vram[cy, (cx + ((w >> ((u & 1) * 8)) & 0xFF)) & 1023]
        else:
            px = self.vram[(ty + v) & 511, (tx + u) & 1023]
        px = px.astype(np.int32)
        return px & 31, (px >> 5) & 31, (px >> 10) & 31, (px >> 15) & 1, px == 0

    def blend(self, ys, xs, src, semi, abr):
        """`semi`: None (opaque) or a per-pixel bool array."""
        dst = self.fb[ys, xs]
        if semi is not None:
            mix = {
                0: (dst + src) >> 1,
                1: dst + src,
                2: dst - src,
                3: dst + (src >> 2),
            }[abr]
            src = np.where(semi[:, None], mix, src)
        self.fb[ys, xs] = np.clip(src, 0, 31)

    def shade(self, ys, xs, cols, uvs, bary, tex, tpage, clut, semi, raw, gour):
        l0, l1, l2 = bary
        if gour:
            c = [l0 * cols[0][k] + l1 * cols[1][k] + l2 * cols[2][k] for k in range(3)]
        else:
            c = [np.full(l0.shape, cols[0][k], dtype=float) for k in range(3)]
        if not tex:
            src = np.stack([c[k].astype(np.int32) >> 3 for k in range(3)], -1)
            sm = np.ones(len(ys), bool) if semi else None
            self.blend(ys, xs, src, sm, (self.tpage >> 5) & 3)
            return
        u = np.floor(l0 * uvs[0][0] + l1 * uvs[1][0] + l2 * uvs[2][0] + 1e-6).astype(np.int32)
        v = np.floor(l0 * uvs[0][1] + l1 * uvs[1][1] + l2 * uvs[2][1] + 1e-6).astype(np.int32)
        r, g, b, stp, transparent = self.texel(tpage, clut, u, v)
        t = [r, g, b]
        if not raw:
            t = [np.minimum((t[k] * c[k].astype(np.int32)) >> 7, 31) for k in range(3)]
        keep = ~transparent
        src = np.stack([t[k][keep] for k in range(3)], -1)
        sm = stp[keep].astype(bool) if semi else None
        self.blend(ys[keep], xs[keep], src, sm, (tpage >> 5) & 3)

    def triangle(self, vs, cols, uvs, **kw):
        xs = [p[0] for p in vs]
        ys = [p[1] for p in vs]
        if max(xs) - min(xs) >= 1024 or max(ys) - min(ys) >= 512:
            return 0
        x0, x1 = max(min(xs), 0), min(max(xs), W - 1)
        y0, y1 = max(min(ys), 0), min(max(ys), H - 1)
        if x0 > x1 or y0 > y1:
            return 0
        (ax, ay), (bx, by), (cx, cy) = vs
        area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
        if area == 0:
            return 0
        if area < 0:
            vs = [vs[0], vs[2], vs[1]]
            cols = [cols[0], cols[2], cols[1]]
            uvs = [uvs[0], uvs[2], uvs[1]]
            (ax, ay), (bx, by), (cx, cy) = vs
            area = -area
        gy, gx = np.mgrid[y0 : y1 + 1, x0 : x1 + 1]
        w0 = (cx - bx) * (gy - by) - (cy - by) * (gx - bx)
        w1 = (ax - cx) * (gy - cy) - (ay - cy) * (gx - cx)
        w2 = (bx - ax) * (gy - ay) - (by - ay) * (gx - ax)
        inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
        # Top-left fill rule: a pixel on a right or bottom edge is not drawn.
        for w, (px, py), (qx, qy) in (
            (w2, (ax, ay), (bx, by)),
            (w0, (bx, by), (cx, cy)),
            (w1, (cx, cy), (ax, ay)),
        ):
            dx, dy = qx - px, qy - py
            if not (dy < 0 or (dy == 0 and dx > 0)):
                inside &= w != 0
        if not inside.any():
            return 0
        bary = (w0[inside] / area, w1[inside] / area, w2[inside] / area)
        self.shade(gy[inside], gx[inside], cols, uvs, bary, **kw)
        return int(inside.sum())

    def sprite(self, x, y, w, h, col, tex, clut, u0, v0, semi, raw):
        xa, xb = max(x, 0), min(x + w, W)
        ya, yb = max(y, 0), min(y + h, H)
        if xa >= xb or ya >= yb:
            return
        gy, gx = np.mgrid[ya:yb, xa:xb]
        ys, xs = gy.ravel(), gx.ravel()
        abr = (self.tpage >> 5) & 3
        if not tex:
            src = np.tile(np.array([c >> 3 for c in col], np.int32), (len(ys), 1))
            self.blend(ys, xs, src, np.ones(len(ys), bool) if semi else None, abr)
            return
        r, g, b, stp, transparent = self.texel(self.tpage, clut, u0 + xs - x, v0 + ys - y)
        t = [r, g, b]
        if not raw:
            t = [np.minimum((t[k] * col[k]) >> 7, 31) for k in range(3)]
        keep = ~transparent
        src = np.stack([t[k][keep] for k in range(3)], -1)
        self.blend(ys[keep], xs[keep], src, stp[keep].astype(bool) if semi else None, abr)


def s11(v: int) -> int:
    return ((v & 0x7FF) ^ 0x400) - 0x400


def rgb(w: int):
    return (w & 0xFF, (w >> 8) & 0xFF, (w >> 16) & 0xFF)


def xy(w: int):
    return (s11(w & 0xFFFF), s11(w >> 16))


def walk(r: Raster, record: int, wanted, log):
    ot = r.u32(DOUBLE_BUFFER + record * DOUBLE_BUFFER_STRIDE + DOUBLE_BUFFER_OT)
    a = ot + (OT_BUCKETS - 1) * 4
    bucket = OT_BUCKETS - 1
    for _ in range(200_000):
        if ot <= a < ot + OT_BUCKETS * 4:
            bucket = (a - ot) // 4
        tag = r.u32(a)
        p, end = a + 4, a + 4 + (tag >> 24) * 4
        while p < end:
            c = r.u32(p)
            cmd = c >> 24
            if 0x20 <= cmd < 0x40:
                gour, quad, tex = bool(cmd & 0x10), bool(cmd & 8), bool(cmd & 4)
                semi, raw = bool(cmd & 2), bool(cmd & 1)
                vs, cols, uvs = [], [], []
                clut, tpage, col, q = 0, r.tpage, rgb(c), p + 4
                for k in range(4 if quad else 3):
                    if gour and k > 0:
                        col = rgb(r.u32(q))
                        q += 4
                    cols.append(col)
                    vs.append(xy(r.u32(q)))
                    q += 4
                    if tex:
                        t = r.u32(q)
                        q += 4
                        uvs.append((t & 0xFF, (t >> 8) & 0xFF))
                        clut = t >> 16 if k == 0 else clut
                        tpage = t >> 16 if k == 1 else tpage
                    else:
                        uvs.append((0, 0))
                if wanted(bucket, clut if tex else None):
                    kw = dict(tex=tex, tpage=tpage, clut=clut, semi=semi, raw=raw, gour=gour)
                    px = r.triangle(vs[:3], cols[:3], uvs[:3], **kw)
                    if quad:
                        px += r.triangle(vs[1:], cols[1:], uvs[1:], **kw)
                    log.append((bucket, f"{cmd:#04x}", tex and f"{clut:#06x}", vs, px))
                if tex:
                    # A textured polygon's page word loads the draw mode.
                    r.tpage = (r.tpage & ~0x9FF) | (tpage & 0x9FF)
                p = q
            elif 0x60 <= cmd < 0x80:
                tex, semi, raw = bool(cmd & 4), bool(cmd & 2), bool(cmd & 1)
                size = (cmd >> 3) & 3
                x, y = xy(r.u32(p + 4))
                q, clut, u0, v0 = p + 8, 0, 0, 0
                if tex:
                    t = r.u32(q)
                    q += 4
                    u0, v0, clut = t & 0xFF, (t >> 8) & 0xFF, t >> 16
                if size == 0:
                    t = r.u32(q)
                    q += 4
                    w, h = t & 0x3FF, (t >> 16) & 0x1FF
                else:
                    w = h = {1: 1, 2: 8, 3: 16}[size]
                if wanted(bucket, clut if tex else None) and w and h:
                    r.sprite(x, y, w, h, rgb(c), tex, clut, u0, v0, semi, raw)
                    log.append((bucket, f"{cmd:#04x}", tex and f"{clut:#06x}", [(x, y), (w, h)], w * h))
                p = q
            elif cmd == 0xE1:
                r.tpage = c & 0xFF_FFFF
                p += 4
            elif cmd == 0x02:
                p += 12
            elif 0x40 <= cmd < 0x60:
                if cmd & 8:
                    q = p + 4
                    while q < end and (r.u32(q) & 0xF000_F000) != 0x5000_5000:
                        q += 4
                    p = q + 4
                else:
                    p += 4 * (4 if cmd & 0x10 else 3)
            else:
                p += 4
        if (tag & 0xFF_FFFF) == 0xFF_FFFF:
            break
        a = (tag & 0xFF_FFFF) | KSEG0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("ram", help="raw main-RAM image (0x80000000..0x80200000)")
    ap.add_argument("vram", help="raw 1 MiB VRAM blob (vram-dump --out-bin)")
    ap.add_argument("--record", type=int, default=0, choices=(0, 1))
    ap.add_argument("--out", default="ot.png")
    ap.add_argument("--buckets", help="LO:HI, inclusive")
    ap.add_argument("--clut", help="draw only textured packets on these CLUT words")
    ap.add_argument("--skip-clut", help="leave out textured packets on these CLUT words")
    ap.add_argument("--list", action="store_true", help="print every drawn packet")
    args = ap.parse_args()

    ram = open(args.ram, "rb").read()
    vram = np.frombuffer(open(args.vram, "rb").read(), dtype="<u2").reshape(512, 1024).copy()
    lo, hi = (int(v) for v in args.buckets.split(":")) if args.buckets else (0, OT_BUCKETS)
    words = lambda s: {int(v, 16) for v in s.split(",")} if s else set()
    only, skip = words(args.clut), words(args.skip_clut)

    def wanted(bucket, clut):
        if not lo <= bucket <= hi:
            return False
        if clut is None:
            return not only
        return clut not in skip and (not only or clut in only)

    r = Raster(ram, vram)
    log = []
    walk(r, args.record, wanted, log)
    # The console's 5-to-8-bit expansion, as the VRAM dumps present it.
    Image.fromarray(((r.fb << 3) | (r.fb >> 2)).astype(np.uint8), "RGB").save(args.out)
    print(f"record {args.record}: {len(log)} packets drawn -> {args.out}")
    if args.list:
        for row in log:
            print(row)
    return 0


if __name__ == "__main__":
    sys.exit(main())
