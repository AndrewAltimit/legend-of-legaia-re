#!/usr/bin/env python3
"""Generate the Rebirth pack's ember crystal model (a glTF 2.0 binary).

The crystal is a stretched octahedron split into two rings of facets, each
facet textured from a 32 x 64 vertical gradient (white-hot core, amber,
deep red rim) drawn here. Everything is the pack's own art: no disc bytes.

Usage: generate_ember_crystal.py OUT.glb
"""

import json
import math
import struct
import sys
import zlib


def png(w, h, rgba):
    raw = b"".join(b"\0" + bytes(rgba[y * w * 4:(y + 1) * w * 4]) for y in range(h))

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def texture():
    w, h = 32, 64
    px = []
    for y in range(h):
        t = abs(y / (h - 1) * 2 - 1)  # 0 at the girdle, 1 at the tips
        for x in range(w):
            edge = abs(x / (w - 1) * 2 - 1)
            k = min(1.0, t * 0.8 + edge * 0.5)
            r = int(255 - 70 * k)
            g = int(235 - 190 * k)
            b = int(170 - 160 * k)
            # Faint vertical striations.
            s = 0.85 + 0.15 * math.sin(x * 1.7 + y * 0.3)
            px += [int(r * s), int(g * s), int(b * s), 255]
    return png(w, h, px)


def main(out):
    sides = 6
    top, bottom, girdle, radius = 0.55, -0.35, 0.0, 0.16
    pos, uv, idx = [], [], []
    for i in range(sides):
        a0 = 2 * math.pi * i / sides
        a1 = 2 * math.pi * (i + 1) / sides
        p0 = [radius * math.cos(a0), girdle, radius * math.sin(a0)]
        p1 = [radius * math.cos(a1), girdle, radius * math.sin(a1)]
        for tip, v_tip in ((top, 0.0), (bottom, 1.0)):
            base = len(pos)
            pos += [p0, p1, [0.0, tip, 0.0]]
            uv += [[0.0, 0.5], [1.0, 0.5], [0.5, v_tip]]
            idx += [base, base + 2, base + 1] if tip > 0 else [base, base + 1, base + 2]
    img = texture()
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
        "asset": {"version": "2.0", "generator": "generate_ember_crystal.py"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "ember_crystal"}],
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


if __name__ == "__main__":
    main(sys.argv[1])
