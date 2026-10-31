#!/usr/bin/env python3
"""Generate the Rebirth pack's set-piece models (glTF 2.0 binaries).

Each model is built here from primitive solids (prisms, discs, tori,
gears, cloth strips) and textured from a small banded strip drawn here:
every band is a two-colour gradient with a little seeded grain, and each
part of a model maps into its own band. Everything is the pack's own art:
no disc bytes.

One glTF unit is 100 field world units (a map tile is 128), +Y up.

Usage: generate_rebirth_props.py OUT_DIR
"""

import json
import math
import os
import random
import struct
import sys
import zlib


def png(w, h, rgba):
    raw = b"".join(b"\0" + bytes(rgba[y * w * 4:(y + 1) * w * 4]) for y in range(h))

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def strip(bands, seed, w=32, band_h=16, stripes=0.0):
    """A texture of `bands` (each `(top_rgb, bottom_rgb)`) stacked top to
    bottom; returns the PNG and each band's `(v0, v1)`."""
    rnd = random.Random(seed)
    h = band_h * len(bands)
    px = []
    for b, (c0, c1) in enumerate(bands):
        for y in range(band_h):
            t = y / (band_h - 1)
            for x in range(w):
                g = 0.9 + 0.1 * rnd.random()
                if stripes:
                    g *= 1.0 - stripes * (0.5 + 0.5 * math.sin(x * 0.8))
                px += [max(0, min(255, int((c0[i] * (1 - t) + c1[i] * t) * g))) for i in range(3)] + [255]
    uv = [((b * band_h + 0.5) / h, ((b + 1) * band_h - 0.5) / h) for b in range(len(bands))]
    return png(w, h, px), uv


class Mesh:
    def __init__(self):
        self.pos, self.uv, self.idx = [], [], []

    def tri(self, a, b, c, ta, tb, tc):
        base = len(self.pos)
        self.pos += [a, b, c]
        self.uv += [ta, tb, tc]
        self.idx += [base, base + 1, base + 2]

    def quad(self, a, b, c, d, band, both=False):
        """Corners in order round the face (counter-clockwise seen from the
        front, glTF's winding); `band` is `(v0, v1)`."""
        v0, v1 = band
        self.tri(a, b, c, [0.0, v1], [1.0, v1], [1.0, v0])
        self.tri(a, c, d, [0.0, v1], [1.0, v0], [0.0, v0])
        if both:
            self.tri(a, c, b, [0.0, v1], [1.0, v0], [1.0, v1])
            self.tri(a, d, c, [0.0, v1], [0.0, v0], [1.0, v0])

    def prism(self, n, r0, r1, y0, y1, band, cx=0.0, cz=0.0, twist=0.0, cap=None):
        """A frustum of `n` sides from radius `r0` at `y0` to `r1` at `y1`."""
        def ring(r, y, k):
            a = 2 * math.pi * k / n + twist
            return [cx + r * math.cos(a), y, cz + r * math.sin(a)]
        for k in range(n):
            a, b = ring(r0, y0, k), ring(r0, y0, k + 1)
            c, d = ring(r1, y1, k + 1), ring(r1, y1, k)
            self.quad(a, d, c, b, band)
        if cap is not None:
            v0, v1 = cap
            top = [cx, y1, cz]
            for k in range(n):
                self.tri(top, ring(r1, y1, k + 1), ring(r1, y1, k), [0.5, v0], [0.0, v1], [1.0, v1])

    def torus(self, n, m, R, r, y, band):
        for i in range(n):
            for j in range(m):
                def p(i, j):
                    a = 2 * math.pi * i / n
                    b = 2 * math.pi * j / m
                    return [(R + r * math.cos(b)) * math.cos(a), y + r * math.sin(b),
                            (R + r * math.cos(b)) * math.sin(a)]
                self.quad(p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1), band)

    def box(self, x0, y0, z0, x1, y1, z1, band):
        c = [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1],
             [x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]]
        for f in ((0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7), (4, 5, 6, 7), (3, 2, 1, 0)):
            self.quad(*[c[i] for i in f], band)

    def gear(self, teeth, r_in, r_out, y0, y1, band, cap):
        n = teeth * 4
        def r(k):
            return r_out if (k % 4) in (1, 2) else r_in
        def pt(k, y):
            a = 2 * math.pi * k / n
            return [r(k) * math.cos(a), y, r(k) * math.sin(a)]
        for k in range(n):
            self.quad(pt(k, y0), pt(k, y1), pt(k + 1, y1), pt(k + 1, y0), band)
        for y, flip in ((y1, False), (y0, True)):
            c = [0.0, y, 0.0]
            for k in range(n):
                a, b = pt(k, y), pt(k + 1, y)
                if flip:
                    self.tri(c, a, b, [0.5, cap[0]], [0.0, cap[1]], [1.0, cap[1]])
                else:
                    self.tri(c, b, a, [0.5, cap[0]], [1.0, cap[1]], [0.0, cap[1]])


def write_glb(out, mesh, img, name):
    pos, uv, idx = mesh.pos, mesh.uv, mesh.idx
    assert len(pos) < 65536
    bin_ = b"".join(struct.pack("<3f", *p) for p in pos)
    uv_at = len(bin_)
    bin_ += b"".join(struct.pack("<2f", *t) for t in uv)
    idx_at = len(bin_)
    bin_ += b"".join(struct.pack("<H", i) for i in idx)
    while len(bin_) % 4:
        bin_ += b"\0"
    img_at = len(bin_)
    bin_ += img
    while len(bin_) % 4:
        bin_ += b"\0"
    doc = {
        "asset": {"version": "2.0", "generator": "generate_rebirth_props.py"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": name}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "TEXCOORD_0": 1}, "indices": 2, "material": 0}]}],
        "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}],
        "textures": [{"source": 0}],
        "images": [{"bufferView": 3, "mimeType": "image/png"}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": len(pos), "type": "VEC3",
             "min": [min(p[i] for p in pos) for i in range(3)], "max": [max(p[i] for p in pos) for i in range(3)]},
            {"bufferView": 1, "componentType": 5126, "count": len(uv), "type": "VEC2"},
            {"bufferView": 2, "componentType": 5123, "count": len(idx), "type": "SCALAR"},
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": uv_at},
            {"buffer": 0, "byteOffset": uv_at, "byteLength": idx_at - uv_at},
            {"buffer": 0, "byteOffset": idx_at, "byteLength": len(idx) * 2},
            {"buffer": 0, "byteOffset": img_at, "byteLength": len(img)},
        ],
        "buffers": [{"byteLength": len(bin_)}],
    }
    js = json.dumps(doc, separators=(",", ":")).encode()
    while len(js) % 4:
        js += b" "
    total = 12 + 8 + len(js) + 8 + len(bin_)
    with open(out, "wb") as f:
        f.write(b"glTF" + struct.pack("<II", 2, total))
        f.write(struct.pack("<I", len(js)) + b"JSON" + js)
        f.write(struct.pack("<I", len(bin_)) + b"BIN\0" + bin_)


STONE = ((168, 160, 140), (96, 90, 80))
DARK_STONE = ((110, 104, 96), (52, 48, 46))
BRASS = ((255, 222, 130), (150, 96, 30))
COPPER = ((240, 150, 90), (110, 52, 24))
IRON = ((150, 156, 168), (60, 62, 70))
GLOW_BLUE = ((230, 250, 255), (70, 150, 255))
GLOW_GREEN = ((230, 255, 220), (60, 200, 110))
EMBER = ((255, 240, 170), (220, 70, 20))
CLOTH_RED = ((220, 60, 60), (110, 20, 30))
CLOTH_GOLD = ((250, 210, 90), (170, 110, 30))
PAPER = ((255, 236, 180), (240, 150, 60))
WATER = ((200, 240, 255), (40, 110, 200))
WOOD = ((176, 124, 72), (92, 58, 30))
DARK_WOOD = ((120, 82, 48), (58, 36, 20))
APPLE = ((236, 80, 50), (140, 30, 20))
PEAR = ((214, 210, 90), (120, 130, 40))


def sundial(out):
    """3F: a stepped stone plinth, a brass dial with twelve hour studs and a
    leaning gnomon."""
    img, b = strip([STONE, DARK_STONE, BRASS, COPPER], 31)
    m = Mesh()
    m.prism(8, 0.62, 0.56, 0.0, 0.12, b[1], cap=b[0])
    m.prism(8, 0.46, 0.40, 0.12, 0.42, b[0], cap=b[0])
    m.prism(16, 0.52, 0.52, 0.42, 0.48, b[2], cap=b[2])
    for k in range(12):
        a = 2 * math.pi * k / 12
        m.prism(4, 0.035, 0.035, 0.48, 0.52, b[3], cx=0.44 * math.cos(a), cz=0.44 * math.sin(a), cap=b[3])
    # The gnomon: a thin wedge leaning north.
    m.quad([0.0, 0.48, -0.03], [0.0, 0.48, 0.03], [0.34, 0.86, 0.03], [0.34, 0.86, -0.03], b[3], both=True)
    m.quad([0.0, 0.48, 0.03], [0.36, 0.48, 0.03], [0.34, 0.86, 0.03], [0.0, 0.48, 0.03], b[3], both=True)
    m.quad([0.0, 0.48, -0.03], [0.36, 0.48, -0.03], [0.34, 0.86, -0.03], [0.0, 0.48, -0.03], b[3], both=True)
    write_glb(out, m, img, "sundial")


def ring(out):
    """An armillary ring: a brass torus with eight studs (4F, 7F)."""
    img, b = strip([BRASS, COPPER], 41)
    m = Mesh()
    m.torus(32, 6, 0.9, 0.05, 0.0, b[0])
    for k in range(8):
        a = 2 * math.pi * k / 8
        m.prism(6, 0.08, 0.08, -0.08, 0.08, b[1], cx=0.9 * math.cos(a), cz=0.9 * math.sin(a), cap=b[1])
    write_glb(out, m, img, "armillary_ring")


def obelisk(out):
    """7F: a tall dark obelisk with glowing rune bands and a capstone."""
    img, b = strip([DARK_STONE, GLOW_BLUE, STONE], 71, stripes=0.4)
    m = Mesh()
    m.prism(4, 0.34, 0.32, 0.0, 0.2, b[2], twist=math.pi / 4, cap=b[2])
    m.prism(4, 0.22, 0.16, 0.2, 2.2, b[0], twist=math.pi / 4)
    for y in (0.6, 1.2, 1.8):
        m.prism(4, 0.205, 0.195, y, y + 0.12, b[1], twist=math.pi / 4)
    m.prism(4, 0.16, 0.0, 2.2, 2.55, b[1], twist=math.pi / 4)
    write_glb(out, m, img, "obelisk")


def cog(out):
    """6F: an iron cogwheel with a copper hub."""
    img, b = strip([IRON, COPPER], 61)
    m = Mesh()
    m.gear(14, 0.82, 1.0, -0.08, 0.08, b[0], b[0])
    m.prism(10, 0.24, 0.24, -0.14, 0.14, b[1], cap=b[1])
    for k in range(5):
        a = 2 * math.pi * k / 5
        x0, z0 = 0.2 * math.cos(a), 0.2 * math.sin(a)
        x1, z1 = 0.84 * math.cos(a), 0.84 * math.sin(a)
        nx, nz = -math.sin(a) * 0.06, math.cos(a) * 0.06
        m.quad([x0 - nx, 0.06, z0 - nz], [x1 - nx, 0.06, z1 - nz], [x1 + nx, 0.06, z1 + nz], [x0 + nx, 0.06, z0 + nz], b[0], both=True)
        m.quad([x0 - nx, -0.06, z0 - nz], [x1 - nx, -0.06, z1 - nz], [x1 + nx, -0.06, z1 + nz], [x0 + nx, -0.06, z0 + nz], b[0], both=True)
    write_glb(out, m, img, "cog")


def banner(out, cloth, name):
    """A hanging banner on a brass rod, its hem cut in a swallowtail."""
    img, b = strip([cloth, BRASS, CLOTH_GOLD], 51)
    m = Mesh()
    m.prism(6, 0.025, 0.025, -0.36, 0.36, b[1], cap=b[1])
    # The rod lies along X: rotate the prism's points.
    m.pos = [[p[1], 1.6 + p[0], p[2]] for p in m.pos]
    w, top = 0.32, 1.58
    rows = 6
    for r in range(rows):
        y0 = top - r * 0.2
        y1 = y0 - 0.2
        sway = lambda y: 0.02 * math.sin(y * 7.0)
        m.quad([-w, y1, sway(y1)], [w, y1, sway(y1)], [w, y0, sway(y0)], [-w, y0, sway(y0)], b[0], both=True)
    y = top - rows * 0.2
    m.quad([-w, y, 0.0], [w, y, 0.0], [w, y + 0.06, 0.0], [-w, y + 0.06, 0.0], b[2], both=True)
    m.tri([-w, y, 0.0], [0.0, y + 0.0, 0.0], [-w, y - 0.22, 0.0], [0.0, b[0][0]], [1.0, b[0][0]], [0.0, b[0][1]])
    m.tri([-w, y, 0.0], [-w, y - 0.22, 0.0], [0.0, y, 0.0], [0.0, b[0][0]], [0.0, b[0][1]], [1.0, b[0][0]])
    m.tri([w, y, 0.0], [w, y - 0.22, 0.0], [0.0, y, 0.0], [0.0, b[0][0]], [0.0, b[0][1]], [1.0, b[0][0]])
    m.tri([w, y, 0.0], [0.0, y, 0.0], [w, y - 0.22, 0.0], [0.0, b[0][0]], [1.0, b[0][0]], [0.0, b[0][1]])
    # A gold emblem: a diamond on the cloth's face, both sides.
    c = top - 0.55
    for z in (0.03, -0.03):
        m.quad([0.0, c - 0.16, z], [0.12, c, z], [0.0, c + 0.16, z], [-0.12, c, z], b[2], both=True)
    write_glb(out, m, img, name)


def lantern(out):
    """5F: a paper lantern with brass caps and a tassel."""
    img, b = strip([PAPER, BRASS, CLOTH_RED], 55, stripes=0.25)
    m = Mesh()
    prof = [(0.0, 0.10), (0.08, 0.20), (0.18, 0.25), (0.30, 0.25), (0.40, 0.20), (0.48, 0.10)]
    for (y0, r0), (y1, r1) in zip(prof, prof[1:]):
        m.prism(10, r0, r1, y0, y1, b[0])
    m.prism(10, 0.11, 0.11, 0.48, 0.53, b[1], cap=b[1])
    m.prism(10, 0.11, 0.11, -0.05, 0.0, b[1], cap=b[1])
    m.prism(4, 0.02, 0.0, -0.05, -0.28, b[2])
    write_glb(out, m, img, "lantern")


def lamp_post(out):
    """The Isles: a timber post with a crossarm and a paper lantern hung
    from its end; its foot stands on the ground at the origin."""
    img, b = strip([WOOD, DARK_WOOD, PAPER, BRASS], 57)
    m = Mesh()
    m.prism(4, 0.07, 0.06, 0.0, 1.62, b[0], twist=math.pi / 4, cap=b[1])
    m.prism(4, 0.10, 0.08, 0.0, 0.12, b[1], twist=math.pi / 4, cap=b[1])
    m.box(-0.04, 1.48, -0.035, 0.42, 1.55, 0.035, b[1])
    m.box(0.33, 1.36, -0.01, 0.35, 1.48, 0.01, b[3])
    prof = [(1.06, 0.06), (1.10, 0.11), (1.17, 0.13), (1.27, 0.13), (1.33, 0.10), (1.36, 0.06)]
    for (y0, r0), (y1, r1) in zip(prof, prof[1:]):
        m.prism(8, r0, r1, y0, y1, b[2], cx=0.34)
    m.prism(8, 0.065, 0.065, 1.36, 1.39, b[3], cx=0.34, cap=b[3])
    m.prism(8, 0.065, 0.065, 1.03, 1.06, b[3], cx=0.34, cap=b[3])
    write_glb(out, m, img, "lamp_post")


def produce_crate(out):
    """An open slatted crate heaped with fruit, for a stall top or a stack
    on the ground: 0.5 x 0.36, 0.26 high, its base at the origin."""
    img, b = strip([WOOD, DARK_WOOD, APPLE, PEAR], 67)
    m = Mesh()
    hx, hz, h = 0.25, 0.18, 0.16
    m.box(-hx, 0.0, -hz, hx, 0.02, hz, b[1])
    for (x0, z0, x1, z1) in ((-hx, -hz, hx, -hz + 0.02), (-hx, hz - 0.02, hx, hz),
                             (-hx, -hz, -hx + 0.02, hz), (hx - 0.02, -hz, hx, hz)):
        m.box(x0, 0.0, z0, x1, h, z1, b[0])
    rnd = random.Random(68)
    for i in range(14):
        x = rnd.uniform(-hx + 0.06, hx - 0.06)
        z = rnd.uniform(-hz + 0.06, hz - 0.06)
        y = h - 0.04 + rnd.uniform(0.0, 0.06)
        r = rnd.uniform(0.045, 0.06)
        band = b[2] if i % 3 else b[3]
        m.prism(6, r * 0.6, r, y - r * 0.5, y, band, cx=x, cz=z)
        m.prism(6, r, r * 0.5, y, y + r * 0.7, band, cx=x, cz=z, cap=band)
    write_glb(out, m, img, "produce_crate")


def fountain(out):
    """3F / 4F: a round basin with a spout column and a water sheet."""
    img, b = strip([STONE, WATER, DARK_STONE], 33)
    m = Mesh()
    m.prism(16, 0.95, 0.9, 0.0, 0.32, b[0], cap=None)
    m.prism(16, 0.84, 0.84, 0.32, 0.0, b[2])
    m.prism(16, 0.86, 0.86, 0.24, 0.24, b[1], cap=b[1])
    m.prism(8, 0.14, 0.12, 0.0, 0.9, b[0], cap=b[0])
    m.prism(12, 0.34, 0.28, 0.9, 1.02, b[0], cap=b[1])
    # A falling sheet of water round the column.
    m.prism(12, 0.32, 0.5, 0.98, 0.26, b[1])
    write_glb(out, m, img, "fountain")


def throne_brazier(out):
    """2F / 8F: a tall iron brazier bowl on a tripod, coals inside."""
    img, b = strip([IRON, EMBER, DARK_STONE], 81)
    m = Mesh()
    for k in range(3):
        a = 2 * math.pi * k / 3
        x, z = 0.3 * math.cos(a), 0.3 * math.sin(a)
        m.quad([x * 1.4 - 0.03, 0.0, z * 1.4], [x * 1.4 + 0.03, 0.0, z * 1.4], [x * 0.5 + 0.03, 0.9, z * 0.5], [x * 0.5 - 0.03, 0.9, z * 0.5], b[0], both=True)
    m.prism(12, 0.18, 0.42, 0.85, 1.15, b[0])
    m.prism(12, 0.40, 0.40, 1.08, 1.08, b[1], cap=b[1])
    m.torus(16, 4, 0.42, 0.03, 1.15, b[0])
    write_glb(out, m, img, "brazier")


SAND_SHELL = ((250, 236, 214), (196, 160, 126))
PINK_SHELL = ((250, 200, 186), (190, 110, 96))
STARFISH = ((250, 150, 80), (190, 70, 40))
BLEACHED = ((214, 204, 184), (128, 116, 98))
HULL_PAINT = ((90, 130, 150), (40, 66, 86))
FROND = ((112, 160, 72), (34, 72, 28))
DUNE_GRASS = ((214, 210, 140), (110, 128, 60))


def loft(m, sections, band, both=False):
    """Quads between consecutive rings of points (each ring the same
    length), `band` mapped along each strip."""
    for s0, s1 in zip(sections, sections[1:]):
        for k in range(len(s0) - 1):
            m.quad(s0[k], s0[k + 1], s1[k + 1], s1[k], band, both=both)


def rowboat(out):
    """The Isles: a clinker rowboat, 2.6 long (+X bow), its keel on the
    ground at the origin, two thwarts and a pair of oars laid inside."""
    img, b = strip([WOOD, DARK_WOOD, HULL_PAINT, BLEACHED], 91)
    m = Mesh()
    n = 10
    length, beam, depth = 2.6, 0.92, 0.42
    sections = []
    for i in range(n + 1):
        t = i / n
        x = (t - 0.5) * length
        # Blunt at the stern (-X), fine at the bow.
        w = beam / 2 * (1 - abs(2 * t - 1) ** (2.4 if t > 0.5 else 3.6)) ** 0.5
        w = max(w, 0.02 if t > 0.5 else 0.16)
        sheer = depth + 0.14 * (2 * t - 1) ** 2
        ring = []
        for k in range(7):
            a = math.pi * k / 6
            ring.append([x, sheer * (1 - 0.98 * math.sin(a)), -w * math.cos(a)])
        sections.append(ring)
    loft(m, sections, b[2], both=True)
    # The gunwale: a dark rail along each side.
    for side in (0, 6):
        for s0, s1 in zip(sections, sections[1:]):
            p0, p1 = s0[side], s1[side]
            m.quad([p0[0], p0[1] - 0.05, p0[2]], [p1[0], p1[1] - 0.05, p1[2]],
                   [p1[0], p1[1] + 0.02, p1[2]], [p0[0], p0[1] + 0.02, p0[2]], b[1], both=True)
    # The transom closing the stern.
    s = sections[0]
    for k in range(len(s) - 1):
        m.tri([s[0][0], s[0][1], 0.0], s[k], s[k + 1], [0.5, b[1][0]], [0.0, b[1][1]], [1.0, b[1][1]])
        m.tri([s[0][0], s[0][1], 0.0], s[k + 1], s[k], [0.5, b[1][0]], [1.0, b[1][1]], [0.0, b[1][1]])
    # Floorboards, two thwarts, and the oars.
    m.box(-0.95, 0.04, -0.2, 0.75, 0.07, 0.2, b[0])
    for x in (-0.45, 0.35):
        m.box(x - 0.09, depth - 0.12, -0.4, x + 0.09, depth - 0.08, 0.4, b[0])
    for z in (-0.12, 0.12):
        m.box(-1.0, depth - 0.07, z - 0.025, 1.05, depth - 0.03, z + 0.025, b[3])
        m.box(0.75, depth - 0.08, z - 0.07, 1.15, depth - 0.05, z + 0.07, b[3])
    write_glb(out, m, img, "rowboat")


def shells(out):
    """The Isles: a scatter of shells - three cones, two fan scallops and a
    starfish - lying in a 0.5 patch round the origin."""
    img, b = strip([SAND_SHELL, PINK_SHELL, STARFISH], 93, stripes=0.35)
    m = Mesh()
    rnd = random.Random(94)
    for _ in range(3):
        cx, cz = rnd.uniform(-0.2, 0.2), rnd.uniform(-0.2, 0.2)
        r = rnd.uniform(0.03, 0.05)
        m.prism(6, r, 0.0, 0.0, r * 2.4, b[0], cx=cx, cz=cz, twist=rnd.uniform(0, 3))
    for _ in range(2):
        cx, cz = rnd.uniform(-0.22, 0.22), rnd.uniform(-0.22, 0.22)
        a0 = rnd.uniform(0, 2 * math.pi)
        r = rnd.uniform(0.06, 0.09)
        ribs = 7
        for k in range(ribs):
            a = a0 + (k / ribs - 0.5) * 2.0
            a1 = a0 + ((k + 1) / ribs - 0.5) * 2.0
            hub = [cx, 0.005, cz]
            p = [cx + r * math.cos(a), 0.035 if k % 2 else 0.02, cz + r * math.sin(a)]
            q = [cx + r * math.cos(a1), 0.02 if k % 2 else 0.035, cz + r * math.sin(a1)]
            m.tri(hub, q, p, [0.5, b[1][0]], [1.0, b[1][1]], [0.0, b[1][1]])
            m.tri(hub, p, q, [0.5, b[1][0]], [0.0, b[1][1]], [1.0, b[1][1]])
    # A five-armed starfish.
    sx, sz = 0.12, -0.16
    pts = []
    for k in range(10):
        a = math.pi * k / 5
        r = 0.11 if k % 2 == 0 else 0.04
        pts.append([sx + r * math.cos(a), 0.012, sz + r * math.sin(a)])
    top = [sx, 0.03, sz]
    for k in range(10):
        p, q = pts[k], pts[(k + 1) % 10]
        m.tri(top, q, p, [0.5, b[2][0]], [1.0, b[2][1]], [0.0, b[2][1]])
        m.tri([p[0], 0.0, p[2]], p, q, [0.0, b[2][1]], [0.0, b[2][0]], [1.0, b[2][0]])
    write_glb(out, m, img, "shells")


def driftwood(out):
    """The Isles: a sea-bleached log 1.7 long lying along X with a bend and
    two broken branch stubs; its underside on the ground at the origin."""
    img, b = strip([BLEACHED, DARK_WOOD], 95, stripes=0.3)
    m = Mesh()
    path = [(-0.85, 0.11, 0.0, 0.10), (-0.3, 0.12, 0.08, 0.12), (0.25, 0.10, 0.02, 0.10),
            (0.85, 0.06, -0.14, 0.05)]
    sections = []
    for (x, y, z, r) in path:
        sections.append([[x, y + r * math.sin(2 * math.pi * k / 8), z + r * math.cos(2 * math.pi * k / 8)]
                         for k in range(9)])
    loft(m, sections, b[0])
    for (x, y, z, r) in (path[0], path[-1]):
        c = [x, y, z]
        ring = [[x, y + r * math.sin(2 * math.pi * k / 8), z + r * math.cos(2 * math.pi * k / 8)] for k in range(9)]
        for k in range(8):
            m.tri(c, ring[k], ring[k + 1], [0.5, b[1][0]], [0.0, b[1][1]], [1.0, b[1][1]])
            m.tri(c, ring[k + 1], ring[k], [0.5, b[1][0]], [1.0, b[1][1]], [0.0, b[1][1]])
    for (x, z, a, ln) in ((-0.15, 0.1, 0.7, 0.42), (0.45, -0.02, -0.9, 0.3)):
        dx, dz = math.cos(a) * ln * 0.6, math.sin(a) * ln
        m.prism(5, 0.04, 0.015, 0.12, 0.2, b[0], cx=x, cz=z)
        m.quad([x, 0.14, z], [x + dx, 0.2, z + dz], [x + dx, 0.23, z + dz], [x, 0.19, z], b[0], both=True)
    write_glb(out, m, img, "driftwood")


def frond_plant(out, name, colour, seed, fronds, length, rise, width):
    """A plant of `fronds` arching blades round the origin, each a strip of
    quads drawn from both sides, so it reads from any side: unlike a flat
    card, never edge-on."""
    img, b = strip([colour, DARK_WOOD], seed)
    m = Mesh()
    rnd = random.Random(seed)
    for f in range(fronds):
        a = 2 * math.pi * (f + rnd.uniform(-0.3, 0.3)) / fronds
        ln = length * rnd.uniform(0.75, 1.1)
        up = rise * rnd.uniform(0.8, 1.15)
        steps = 5
        prev = None
        for s in range(steps + 1):
            t = s / steps
            r = ln * t
            y = up * math.sin(math.pi * 0.85 * t) + 0.01
            w = width * math.sin(math.pi * min(1.0, t * 1.1 + 0.05)) * (1 - 0.6 * t)
            cx, cz = r * math.cos(a), r * math.sin(a)
            nx, nz = -math.sin(a) * w, math.cos(a) * w
            cur = ([cx - nx, y, cz - nz], [cx + nx, y, cz + nz])
            if prev:
                v0 = b[0][0] + (b[0][1] - b[0][0]) * (s - 1) / steps
                v1 = b[0][0] + (b[0][1] - b[0][0]) * s / steps
                m.quad(prev[0], prev[1], cur[1], cur[0], (v1, v0), both=True)
            prev = cur
    write_glb(out, m, img, name)


NET = ((150, 132, 96), (70, 58, 40))


def net_rack(out):
    """The Isles: a fishers' drying rack - two forked posts and a pole 1.8
    long across them along X, a net hung over the pole in sagging folds;
    the posts' feet on the ground at the origin."""
    img, b = strip([DARK_WOOD, WOOD, NET], 101, stripes=0.6)
    m = Mesh()
    for x in (-0.85, 0.85):
        m.prism(5, 0.045, 0.035, 0.0, 1.05, b[0], cx=x, cap=b[0])
        for dz in (-0.05, 0.05):
            m.quad([x, 0.98, 0.0], [x, 0.98, 0.0], [x, 1.12, dz], [x, 1.12, dz * 0.6], b[0], both=True)
    # The pole: a prism built along Y, then laid along X at the forks.
    at = len(m.pos)
    m.prism(6, 0.03, 0.03, -0.95, 0.95, b[1], cap=b[1])
    m.pos[at:] = [[p[1], 1.05 + p[0], p[2]] for p in m.pos[at:]]
    # The net: hung over the pole, falling both sides in folds to a hem.
    cols = 12
    for side in (-1, 1):
        for k in range(cols):
            x0 = -0.8 + 1.6 * k / cols
            x1 = -0.8 + 1.6 * (k + 1) / cols
            sag0 = 0.08 * math.sin(math.pi * k / cols)
            sag1 = 0.08 * math.sin(math.pi * (k + 1) / cols)
            fold0 = 0.03 * (k % 2)
            fold1 = 0.03 * ((k + 1) % 2)
            top0, top1 = [x0, 1.05, 0.0], [x1, 1.05, 0.0]
            bot0 = [x0, 0.35 + sag0, side * (0.2 + fold0)]
            bot1 = [x1, 0.35 + sag1, side * (0.2 + fold1)]
            m.quad(bot0, bot1, top1, top0, b[2], both=True)
    write_glb(out, m, img, "net_rack")


def main(out_dir):
    os.makedirs(out_dir, exist_ok=True)
    sundial(os.path.join(out_dir, "sundial.glb"))
    ring(os.path.join(out_dir, "armillary_ring.glb"))
    obelisk(os.path.join(out_dir, "obelisk.glb"))
    cog(os.path.join(out_dir, "cog.glb"))
    banner(os.path.join(out_dir, "banner_red.glb"), CLOTH_RED, "banner_red")
    lantern(os.path.join(out_dir, "lantern.glb"))
    fountain(os.path.join(out_dir, "fountain.glb"))
    throne_brazier(os.path.join(out_dir, "brazier.glb"))
    lamp_post(os.path.join(out_dir, "lamp_post.glb"))
    produce_crate(os.path.join(out_dir, "produce_crate.glb"))
    rowboat(os.path.join(out_dir, "rowboat.glb"))
    shells(os.path.join(out_dir, "shells.glb"))
    driftwood(os.path.join(out_dir, "driftwood.glb"))
    frond_plant(os.path.join(out_dir, "fern.glb"), "fern", FROND, 97, 9, 0.55, 0.32, 0.09)
    frond_plant(os.path.join(out_dir, "dune_grass.glb"), "dune_grass", DUNE_GRASS, 99, 14, 0.38, 0.42, 0.025)
    net_rack(os.path.join(out_dir, "net_rack.glb"))


if __name__ == "__main__":
    main(sys.argv[1])
