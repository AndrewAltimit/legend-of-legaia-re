#!/usr/bin/env python3
"""Give Lu Delilas twintails - a surgical edit of her exported model.

Demonstrates the OBJ-level modding loop on a retail mesh: export Lu with
`legaia-patcher monster-model --id 164 --export lu`, run this script on the
exported OBJ, and re-import the result. Everything about her stays retail -
body, texture page, palettes, animations - except two tapered hair tails
appended to the head part (`o part_00`), UV-mapped onto her own pink
hair-tuft texels (the pointed "petal" piece her bob already uses) and
riding her hair palette, so the texture page is not modified at all.

The tails are new geometry only; no Sony bytes live in this script, and
its output stays local (the exported OBJ it reads is disc-derived).

Usage:
  legaia-patcher monster-model --input DISC.bin --id 164 --export lu
  python3 scripts/models/lu_twintails_mod.py --stem lu     # -> lu_twintails.obj
  legaia-patcher monster-model --input DISC.bin --id 164 \\
      --obj lu_twintails.obj --texture lu.png --output patched.bin
"""

import argparse

from PIL import Image

# Her hair-tuft "petal" piece (pointed strand art, palette p01) lives in
# this texel neighbourhood; the exact tail UVs are harvested from her OWN
# petal faces at runtime, so every texel the tails sample is already owned
# by the hair palette - the import cannot drag foreign colours into it.
PETAL_ZONE = (2, 60, 48, 118)  # texel bbox the petal faces sit in
HAIR_MTL = "skin_p01"


def harvest_petal_uvs(lines, png_path):
    """Find her petal faces and return (quad_uvs, tip_uv) in texel coords:
    the largest petal face's corners (tl, tr, br, bl) and the lowest point
    of the whole petal (the strand tip)."""
    uvs, cur, mtl = [], None, None
    petal_faces = []
    for line in lines:
        t = line.split()
        if not t:
            continue
        if t[0] == "vt":
            uvs.append((float(t[1]), float(t[2])))
        elif t[0] == "o":
            cur = t[1]
        elif t[0] == "usemtl":
            mtl = t[1]
        elif t[0] == "f" and cur == "part_00" and mtl == HAIR_MTL:
            pts = []
            for w in t[1:]:
                u, v = uvs[int(w.split("/")[1]) - 1]
                pts.append((u * 256 - 0.5, (1 - v) * 256 - 0.5))
            cx = sum(x for x, _ in pts) / len(pts)
            cy = sum(y for _, y in pts) / len(pts)
            zx0, zy0, zx1, zy1 = PETAL_ZONE
            if zx0 <= cx <= zx1 and zy0 <= cy <= zy1:
                petal_faces.append(pts)
    if not petal_faces:
        raise SystemExit("no petal faces found - was the OBJ exported with palette hints?")

    # Keep only faces whose sampled texels are actually PINK hair - the
    # zone also clips a tan ear/mask fragment whose triangles are large.
    img = Image.open(png_path).convert("RGBA")

    def is_pink(f):
        cx = sum(x for x, _ in f) / len(f)
        cy = sum(y for _, y in f) / len(f)
        r, g, b, a = img.getpixel((int(min(255, max(0, cx))), int(min(255, max(0, cy)))))
        return a > 0 and r > 130 and g < r * 0.72 and b > 80

    pink = [f for f in petal_faces if is_pink(f)]
    if pink:
        petal_faces = pink

    def area(pts):
        xs = [x for x, _ in pts]
        ys = [y for _, y in pts]
        return (max(xs) - min(xs)) * (max(ys) - min(ys))

    quads = [f for f in petal_faces if len(f) == 4]
    if quads:
        quad = max(quads, key=area)
        # Canonical corner order: split by y (top pair / bottom pair), then x.
        by_y = sorted(quad, key=lambda p: p[1])
        top = sorted(by_y[:2], key=lambda p: p[0])
        bot = sorted(by_y[2:], key=lambda p: p[0])
        quad_uvs = (top[0], top[1], bot[1], bot[0])  # tl, tr, br, bl
    else:
        # All-triangle petal: map through the largest triangle, bottom
        # corner doubled - each segment's texture converges like a strand.
        tri = max(petal_faces, key=area)
        by_y = sorted(tri, key=lambda p: p[1])
        top = sorted(by_y[:2], key=lambda p: p[0])
        quad_uvs = (top[0], top[1], by_y[2], by_y[2])
        # The tip must stay INSIDE this same triangle - a uv segment to a
        # point in another art piece rasters across the dead gap between
        # them and drags the checker texels into the hair palette.
        return quad_uvs, by_y[2]
    tip_uv = max(quad_uvs, key=lambda p: p[1])
    return quad_uvs, tip_uv


def vt_line(x, y):
    return f"vt {(x + 0.5) / 256.0:.6f} {1.0 - (y + 0.5) / 256.0:.6f}"


def v_line(p, c):
    return f"v {p[0]:.1f} {p[1]:.1f} {p[2]:.1f} {c:.4f} {c:.4f} {c:.4f}"


def build_tails(quad_uvs, tip_uv):
    """Return (v_lines, vt_lines, faces) - faces as lists of (vi, ti) local
    0-based indices into the returned lists."""
    vs, vts, faces = [], [], []

    def add_v(p, c):
        vs.append(v_line(p, c))
        return len(vs) - 1

    def add_vt(xy):
        vts.append(vt_line(xy[0], xy[1]))
        return len(vts) - 1

    tl, tr, br, bl = quad_uvs

    for sx in (-1, 1):
        # Anchor high on the side-back of the skull (skull spans y -70..-20,
        # x about +-30, z back is negative); segment rings drift out + down,
        # ending in a point just below the shoulders ("goes down a bit").
        ax = sx * 26
        ring = lambda cx, cy, cz, w, d: [
            (cx - w, cy, cz - d),
            (cx + w, cy, cz - d),
            (cx + w, cy, cz + d),
            (cx - w, cy, cz + d),
        ]
        r0 = ring(ax, -54, -16, 9, 11)
        r1 = ring(ax + sx * 8, 8, -20, 11, 13)
        tip = (ax + sx * 17, 66, -26)

        # Per-corner shade variation fakes the directional light her own
        # hair carries (front corners brighter, back darker).
        sh0 = (0.44, 0.50, 0.56, 0.50)
        sh1 = (0.36, 0.42, 0.48, 0.42)
        i0 = [add_v(p, c) for p, c in zip(r0, sh0)]
        i1 = [add_v(p, c) for p, c in zip(r1, sh1)]
        it = add_v(tip, 0.33)

        # Side faces: her petal quad on the body, its strand tip on the end.
        t00 = add_vt(tl)
        t01 = add_vt(tr)
        t10 = add_vt(bl)
        t11 = add_vt(br)
        tt = add_vt(tip_uv)
        for k in range(4):
            j = (k + 1) % 4
            faces.append([(i0[k], t00), (i0[j], t01), (i1[j], t11), (i1[k], t10)])
            faces.append([(i1[k], t10), (i1[j], t11), (it, tt)])
        # Top cap so the anchor doesn't show a hole against the skull.
        faces.append([(i0[3], t00), (i0[2], t01), (i0[1], t11), (i0[0], t10)])

    return vs, vts, faces


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stem", default="lu", help="input stem (reads <stem>.obj)")
    ap.add_argument("--out", default=None, help="output OBJ (default <stem>_twintails.obj)")
    args = ap.parse_args()
    out_path = args.out or f"{args.stem}_twintails.obj"

    lines = open(f"{args.stem}.obj").read().splitlines()

    # Count existing global v / vt so appended indices resolve.
    n_v = sum(1 for l in lines if l.startswith("v "))
    n_vt = sum(1 for l in lines if l.startswith("vt "))

    quad_uvs, tip_uv = harvest_petal_uvs(lines, f"{args.stem}.png")
    vs, vts, faces = build_tails(quad_uvs, tip_uv)

    # Insertion points: new v/vt go right before the first `o` statement
    # (all geometry lines must precede the faces that use them); new faces
    # go at the end of the part_00 block.
    first_o = next(i for i, l in enumerate(lines) if l.startswith("o "))
    part00_end = next(
        i for i, l in enumerate(lines[first_o + 1 :], first_o + 1) if l.startswith("o ")
    )

    f_lines = ["# --- lu_twintails_mod: appended tail geometry ---", f"usemtl {HAIR_MTL}"]
    for f in faces:
        f_lines.append(
            "f " + " ".join(f"{n_v + vi + 1}/{n_vt + ti + 1}" for vi, ti in f)
        )

    out = lines[:first_o] + vs + vts + lines[first_o:part00_end] + f_lines + lines[part00_end:]
    with open(out_path, "w") as fh:
        fh.write("\n".join(out) + "\n")
    print(
        f"wrote {out_path}: +{len(vs)} verts, +{len(faces)} faces on part_00 "
        f"(hair palette {HAIR_MTL}, texture page untouched)"
    )


if __name__ == "__main__":
    main()
