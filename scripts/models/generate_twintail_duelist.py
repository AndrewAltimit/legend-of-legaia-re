#!/usr/bin/env python3
"""Generate the "Twintail Duelist" example custom monster model.

An original low-poly character (silver twintails, bandaged limbs, black
dress, red roses, a dagger) authored for the `legaia-patcher monster-model`
replacement pipeline, rigged to Lu Delilas's 15-part battle skeleton so
every retail animation - including Plasma Strike's staged choreography -
plays on it unmodified.

Part conventions (pinned from the retail mesh + idle stream; y-down,
+z forward; parts are posed INDEPENDENTLY per frame, so each part's
origin-to-extremity extent must stay close to retail or joints gap):

  0 head   (origin neck, geometry up: y -70..10)
  1 torso  (origin waist, geometry up: y -115..0)
  2 skirt  (origin waist, geometry down: y 0..54)
  3/6 upper arm L/R (origin shoulder, +y to elbow ~55)
  4/7 forearm  L/R (origin elbow, +y to wrist ~55)
  5/8 hand     L/R (origin wrist, +y ~33)
  9/12 thigh   L/R (origin hip, +y to knee ~100)
  10/13 shin   L/R (origin knee, +y ~120)
  11/14 foot   L/R (origin ankle, +z forward ~70)

Deterministic: no RNG. Output: twintail_duelist.obj / .mtl / .png next to
--out (default data/models/twintail_duelist/twintail_duelist).

The OBJ uses the codec's conventions (raw GTE units, per-vertex colours
around the 0x80 neutral modulation, tpage-space UVs, `o part_NN` groups).
Faces carry no `_pNN` palette hints - the importer auto-palletizes.
"""

import argparse
import math
import os

from PIL import Image, ImageDraw

W = H = 256  # texture page (must match Lu's retail 256x256 page)

# ---------------------------------------------------------------------------
# Texture painting


def paint_texture():
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def patch(x0, y0, x1, y1, base, streaks=None, horiz=False, n=6):
        """Fill a patch with a base colour + evenly spaced darker streaks."""
        d.rectangle([x0, y0, x1 - 1, y1 - 1], fill=base + (255,))
        if streaks:
            span = (y1 - y0) if horiz else (x1 - x0)
            for i in range(n):
                p = int(span * (i + 0.5) / n)
                if horiz:
                    d.line([x0, y0 + p, x1 - 1, y0 + p], fill=streaks + (255,))
                else:
                    d.line([x0 + p, y0, x0 + p, y1 - 1], fill=streaks + (255,))

    # --- face tile (0,0)-(64,64): skin, one red eye, bandage patch, mask.
    skin = (232, 200, 178)
    patch(0, 0, 64, 64, skin)
    # her RIGHT eye (character +x = our left in UV; mirrored on the model
    # anyway) - big red iris, PS1-pixel style
    d.rectangle([10, 22, 26, 36], fill=(250, 248, 246, 255))  # sclera
    d.rectangle([14, 24, 22, 36], fill=(196, 32, 40, 255))  # iris
    d.rectangle([17, 28, 20, 34], fill=(60, 8, 10, 255))  # pupil
    d.line([8, 18, 28, 16], fill=(72, 60, 60, 255), width=2)  # brow
    # her LEFT eye: white bandage patch with a seam cross
    d.rectangle([38, 18, 58, 40], fill=(240, 238, 232, 255))
    d.line([38, 28, 58, 28], fill=(205, 200, 190, 255))
    d.line([48, 18, 48, 40], fill=(205, 200, 190, 255))
    # mouth mask band with stitch ticks
    d.rectangle([6, 44, 58, 62], fill=(243, 241, 236, 255))
    for x in range(10, 58, 8):
        d.line([x, 48, x, 58], fill=(210, 205, 196, 255))

    # --- plain skin (64,0)-(96,32)
    patch(64, 0, 96, 32, skin)

    # --- hair silver (128,0)-(192,64): vertical strand streaks
    patch(128, 0, 192, 64, (208, 210, 218), streaks=(168, 172, 184), n=8)
    d.line([136, 0, 136, 63], fill=(130, 134, 148, 255))
    d.line([168, 0, 168, 63], fill=(130, 134, 148, 255))

    # --- hair dark (192,0)-(256,64): near-black strands (tail underside)
    patch(192, 0, 256, 64, (52, 52, 62), streaks=(30, 30, 38), n=8)

    # --- dress black (0,64)-(64,128): soft fold lines
    patch(0, 64, 64, 128, (38, 36, 44), streaks=(24, 22, 28), n=5)

    # --- bandage white (64,64)-(128,128): horizontal wrap seams
    patch(64, 64, 128, 128, (238, 234, 226), streaks=(206, 200, 188), horiz=True, n=8)

    # --- glove black (128,64)-(160,96)
    patch(128, 64, 160, 96, (30, 28, 34))

    # --- rose red (160,64)-(192,96): petal swirl
    patch(160, 64, 192, 96, (150, 22, 34))
    d.ellipse([166, 70, 186, 90], outline=(96, 10, 20, 255), width=2)
    d.ellipse([172, 76, 180, 84], fill=(96, 10, 20, 255))

    # --- blade silver (224,64)-(256,96): stepped gradient + edge light
    # (few bands - each palette holds at most 15 colours)
    for band in range(4):
        g = 150 + 30 * band
        d.rectangle([224 + band * 8, 64, 224 + band * 8 + 7, 95], fill=(g, g + 4, g + 10, 255))
    d.line([254, 64, 254, 95], fill=(255, 255, 255, 255))

    # --- boot black (0,128)-(64,160): strap lines
    patch(0, 128, 64, 160, (26, 24, 30), streaks=(50, 46, 56), horiz=True, n=3)

    # --- skirt front (64,128)-(128,192): black with white apron + rose
    patch(64, 128, 128, 192, (38, 36, 44), streaks=(24, 22, 28), n=4)
    d.polygon([(84, 128), (108, 128), (112, 190), (80, 190)], fill=(238, 234, 226, 255))
    d.ellipse([88, 140, 104, 156], fill=(150, 22, 34, 255))
    d.ellipse([93, 145, 99, 151], fill=(96, 10, 20, 255))

    # --- stocking white (128,128)-(160,192): fine wrap + top band
    patch(128, 128, 160, 192, (238, 234, 226), streaks=(212, 206, 194), horiz=True, n=10)
    d.rectangle([128, 128, 159, 136], fill=(38, 36, 44, 255))  # garter band

    return img


# UV patch centres (u0,v0,u1,v1) in texel space for each material region.
UV = {
    "face": (2, 2, 62, 62),
    "skin": (66, 2, 94, 30),
    "hair": (130, 2, 190, 62),
    "hair_dark": (194, 2, 254, 62),
    "dress": (2, 66, 62, 126),
    "bandage": (66, 66, 126, 126),
    "glove": (130, 66, 158, 94),
    "rose": (162, 66, 190, 94),
    "blade": (226, 66, 254, 94),
    "boot": (2, 130, 62, 158),
    "skirt_front": (66, 130, 126, 190),
    "stocking": (130, 130, 158, 190),
}


# ---------------------------------------------------------------------------
# Mesh building


class Model:
    def __init__(self):
        self.parts = [[] for _ in range(15)]  # per part: (verts, uvs, colors)

    def face(self, part, pts, mat, shade=None, uv_rect=None):
        """Add one tri/quad face. `pts` are perimeter-ordered [x,y,z].

        `shade`: per-corner brightness multipliers (1.0 = neutral 0x80).
        `uv_rect`: explicit (u,v) per corner in texel space; default maps the
        face onto the material patch by its dominant plane.
        """
        n = len(pts)
        if shade is None:
            shade = [1.0] * n
        u0, v0, u1, v1 = UV[mat]
        if uv_rect is None:
            # Project onto the patch: use the two axes with the largest span.
            xs = [p[0] for p in pts]
            ys = [p[1] for p in pts]
            zs = [p[2] for p in pts]
            spans = [max(xs) - min(xs), max(ys) - min(ys), max(zs) - min(zs)]
            axes = sorted(range(3), key=lambda a: -spans[a])[:2]
            a, b = axes

            def norm(p, ax):
                lo = min(q[ax] for q in pts)
                hi = max(q[ax] for q in pts)
                return 0.5 if hi - lo < 1e-6 else (p[ax] - lo) / (hi - lo)

            uv_rect = [
                (u0 + norm(p, a) * (u1 - u0), v0 + norm(p, b) * (v1 - v0)) for p in pts
            ]
        cols = [min(1.6, max(0.25, s)) for s in shade]
        self.parts[part].append((pts, uv_rect, cols))

    def box(self, part, x0, x1, y0, y1, z0, z1, mat, mats=None, skip=(), shade=1.0):
        """Axis box. `mats` overrides per side: keys +z,-z,+x,-x,+y,-y.

        y is DOWN, so -y is the top face (brighter), +y the bottom.
        """
        mats = mats or {}
        s_top, s_side, s_bot = 1.12 * shade, 0.92 * shade, 0.62 * shade
        s_front, s_back = 1.02 * shade, 0.75 * shade
        faces = {
            "+z": ([(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)], s_front),
            "-z": ([(x1, y0, z0), (x0, y0, z0), (x0, y1, z0), (x1, y1, z0)], s_back),
            "+x": ([(x1, y0, z1), (x1, y0, z0), (x1, y1, z0), (x1, y1, z1)], s_side),
            "-x": ([(x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0)], s_side),
            "-y": ([(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)], s_top),
            "+y": ([(x0, y1, z1), (x1, y1, z1), (x1, y1, z0), (x0, y1, z0)], s_bot),
        }
        for side, (pts, s) in faces.items():
            if side in skip:
                continue
            self.face(part, pts, mats.get(side, mat), shade=[s] * 4)

    def prism(self, part, cx, cz, y0, y1, r_top, r_bot, mat, sides=6, mats_cap=None, cap_top=True, cap_bot=True, shade=1.0, rz=None):
        """Vertical n-gon frustum. r may be (rx, rz) tuples for oval sections."""

        def ring(y, r):
            rx, rzz = r if isinstance(r, tuple) else (r, r)
            return [
                (
                    cx + rx * math.cos(2 * math.pi * (i + 0.5) / sides),
                    y,
                    cz + rzz * math.sin(2 * math.pi * (i + 0.5) / sides),
                )
                for i in range(sides)
            ]

        top = ring(y0, r_top)
        bot = ring(y1, r_bot)
        for i in range(sides):
            j = (i + 1) % sides
            # Shade by outward direction: front (+z) lighter, back darker.
            ang = 2 * math.pi * (i + 1.0) / sides
            zdir = math.sin(ang)
            s = (0.9 + 0.18 * zdir) * shade
            self.face(
                part,
                [top[i], top[j], bot[j], bot[i]],
                mat,
                shade=[s * 1.06, s * 1.06, s * 0.8, s * 0.8],
            )
        def fan(ring_pts, s):
            # Triangle-fan cap (PSX prims are tris/quads only).
            for i in range(1, sides - 1):
                self.face(
                    part,
                    [ring_pts[0], ring_pts[i], ring_pts[i + 1]],
                    (mats_cap or mat),
                    shade=[s] * 3,
                )

        if cap_top:
            fan(list(reversed(top)), 1.15 * shade)
        if cap_bot:
            fan(bot, 0.6 * shade)


def mirror_part(model, src, dst):
    """Mirror a built part across x=0 into another part slot."""
    for pts, uvs, cols in model.parts[src]:
        mpts = [(-x, y, z) for (x, y, z) in pts]
        model.parts[dst].append((list(reversed(mpts)), list(reversed(uvs)), list(reversed(cols))))


def build():
    m = Model()

    # ----- part 0: head (origin at neck; up = -y; face at +z) -------------
    # skull: big and round (the battle camera is far - silhouette carries)
    m.prism(0, 0, -2, -70, -12, (30, 30), (22, 24), "hair", sides=8, cap_bot=False)
    # face plate over the front lower half
    m.face(
        0,
        [(-22, -54, 27), (22, -54, 27), (18, -14, 24), (-18, -14, 24)],
        "face",
        shade=[1.05, 1.05, 0.95, 0.95],
        uv_rect=[(2, 2), (62, 2), (62, 62), (2, 62)],
    )
    # bangs: three angled flaps over the brow
    for x0, x1, drop in ((-24, -8, 16), (-7, 7, 11), (8, 24, 16)):
        m.face(
            0,
            [(x0, -62, 29), (x1, -62, 29), (x1, -62 + drop, 32), (x0, -62 + drop, 32)],
            "hair",
            shade=[1.1, 1.1, 0.9, 0.9],
        )
    # neck
    m.prism(0, 0, 0, -16, 4, (7, 7), (8, 8), "skin", sides=6, cap_top=False, cap_bot=False)
    # twintail anchors (black ribbons), high on the sides
    for sx in (-1, 1):
        m.box(0, sx * 28 - 6, sx * 28 + 6, -66, -54, -10, 2, "hair_dark")
    # twintails: thick, flaring OUT to the sides then sweeping down - they
    # must read from the front battle camera
    for sx in (-1, 1):
        m.prism(0, sx * 42, -6, -60, -6, (13, 15), (16, 18), "hair", sides=5, cap_top=True, cap_bot=False)
        m.prism(0, sx * 48, -10, -6, 58, (16, 18), (6, 7), "hair", sides=5, cap_top=False, cap_bot=False)
        m.prism(0, sx * 50, -12, 58, 82, (6, 7), (2, 2), "hair_dark", sides=4, cap_top=False)

    # ----- part 1: torso (origin waist; up = -y) --------------------------
    # black bodice: waist to chest (the dress must dominate the read)
    m.prism(1, 0, 0, -72, 2, (27, 17), (21, 13), "dress", sides=8, cap_top=False, cap_bot=False)
    # white bandage chest strip + shoulders
    m.prism(1, 0, -1, -98, -72, (23, 14), (27, 17), "bandage", sides=8, cap_bot=False, mats_cap="bandage")
    # shoulder puffs (black)
    for sx in (-1, 1):
        m.box(1, sx * 24 - 10, sx * 24 + 10, -104, -82, -12, 12, "dress")
    # rose brooch at the chest
    m.box(1, -6, 8, -88, -74, 14, 20, "rose", skip=("-z",))

    # ----- part 2: skirt (origin waist; down = +y) ------------------------
    sides = 8
    for i in range(sides):
        j = i + 1
        a0 = 2 * math.pi * (i + 0.5) / sides
        a1 = 2 * math.pi * (j + 0.5) / sides

        def ring_pt(ang, y, rx, rz):
            return (rx * math.cos(ang), y, rz * math.sin(ang))

        top0 = ring_pt(a0, -2, 24, 16)
        top1 = ring_pt(a1, -2, 24, 16)
        hem0 = ring_pt(a0, 64, 54, 38)
        hem1 = ring_pt(a1, 64, 54, 38)
        zmid = math.sin((a0 + a1) / 2)
        mat = "skirt_front" if zmid > 0.55 else "dress"
        s = 0.9 + 0.16 * zmid
        m.face(2, [top0, top1, hem1, hem0], mat, shade=[s * 1.05, s * 1.05, s * 0.72, s * 0.72])
    # long back tails (the dress's split rear panels)
    for sx in (-1, 1):
        m.face(
            2,
            [(sx * 34, 60, -22), (sx * 8, 62, -34), (sx * 14, 100, -38), (sx * 44, 96, -24)],
            "dress",
            shade=[0.8, 0.8, 0.55, 0.55],
        )

    # ----- part 3: LEFT upper arm (origin shoulder; +y to elbow ~55) ------
    m.prism(3, 0, 0, -6, 56, (12, 12), (9, 9), "bandage", sides=6, cap_top=True, cap_bot=False)

    # ----- part 4: LEFT forearm (origin elbow; +y to wrist ~55) -----------
    m.prism(4, 0, 0, -3, 56, (11, 11), (9, 9), "bandage", sides=6, cap_top=True, cap_bot=False)

    # ----- part 5: LEFT hand (black glove + a red rose) -------------------
    m.box(5, -9, 9, 0, 26, -8, 10, "glove")
    m.box(5, -7, 7, 24, 33, -6, 8, "glove")  # fingers
    m.box(5, -8, 8, 6, 22, 10, 22, "rose")  # clutched rose

    # ----- part 6/7: RIGHT arm = mirror of left ---------------------------
    mirror_part(m, 3, 6)
    mirror_part(m, 4, 7)

    # ----- part 8: RIGHT hand (glove + dagger) ----------------------------
    m.box(8, -9, 9, 0, 26, -8, 10, "glove")
    m.box(8, -7, 7, 24, 33, -6, 8, "glove")
    # dagger: crossguard + a long blade so it reads at battle distance
    m.box(8, -4, 4, 31, 37, -14, 16, "glove")  # guard
    m.box(8, -2, 2, 37, 74, -4, 12, "blade")  # blade
    m.face(
        8,
        [(-2, 74, 12), (2, 74, 12), (0, 88, 4)],
        "blade",
        shade=[1.1, 1.1, 1.2],
    )  # tip

    # ----- part 9: LEFT thigh (origin hip; +y to knee ~100) ---------------
    m.prism(9, 0, 0, -6, 100, (16, 15), (11, 11), "stocking", sides=6, cap_top=True, cap_bot=False)

    # ----- part 10: LEFT shin (origin knee; +y ~120) ----------------------
    m.prism(10, 0, 0, -4, 74, (13, 13), (10, 10), "bandage", sides=6, cap_top=True, cap_bot=False)
    # boot cuff
    m.prism(10, 0, 0, 74, 118, (12, 12), (10, 11), "boot", sides=6, cap_top=False, cap_bot=True)

    # ----- part 11: LEFT foot (origin ankle; +z forward) ------------------
    m.box(11, -9, 9, 2, 24, -16, 32, "boot")
    m.box(11, -7, 7, 12, 24, 32, 54, "boot")  # toe wedge

    # ----- part 12/13/14: RIGHT leg = mirror ------------------------------
    mirror_part(m, 9, 12)
    mirror_part(m, 10, 13)
    mirror_part(m, 11, 14)

    return m


# ---------------------------------------------------------------------------
# OBJ emission (codec conventions: v x y z r g b, vt in 0..1 tpage space)


def emit(model, stem_path):
    stem = os.path.basename(stem_path)
    lines = [
        "# Twintail Duelist - original example model for legaia-patcher monster-model",
        "# Rigged to Lu Delilas's 15-part battle skeleton (raw GTE units, y-down)",
        f"mtllib {stem}.mtl",
    ]
    v_lines, vt_lines, f_chunks = [], [], []
    v_map, vt_map = {}, {}

    def vid(p, c):
        key = (round(p[0]), round(p[1]), round(p[2]), round(c * 512))
        if key not in v_map:
            col = min(1.0, 0.5 * c)  # 0x80 neutral = 0.5 in OBJ space
            v_lines.append(
                f"v {key[0]} {key[1]} {key[2]} {col:.4f} {col:.4f} {col:.4f}"
            )
            v_map[key] = len(v_map) + 1
        return v_map[key]

    def vtid(uv):
        key = (round(uv[0] * 4) / 4, round(uv[1] * 4) / 4)
        if key not in vt_map:
            u = (key[0] + 0.5) / 256.0
            v = 1.0 - (key[1] + 0.5) / 256.0
            vt_lines.append(f"vt {u:.6f} {v:.6f}")
            vt_map[key] = len(vt_map) + 1
        return vt_map[key]

    for part in range(15):
        f_chunks.append(f"o part_{part:02}")
        f_chunks.append("usemtl skin")
        for pts, uvs, cols in model.parts[part]:
            ids = [f"{vid(p, c)}/{vtid(uv)}" for p, uv, c in zip(pts, uvs, cols)]
            f_chunks.append("f " + " ".join(ids))

    obj = "\n".join(lines + v_lines + vt_lines + f_chunks) + "\n"
    mtl = f"newmtl skin\nmap_Kd {stem}.png\n"
    return obj, mtl


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--out",
        default="data/models/twintail_duelist/twintail_duelist",
        help="output stem (writes <stem>.obj/.mtl/.png)",
    )
    args = ap.parse_args()
    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)

    model = build()
    obj, mtl = emit(model, args.out)
    with open(args.out + ".obj", "w") as f:
        f.write(obj)
    with open(args.out + ".mtl", "w") as f:
        f.write(mtl)
    paint_texture().save(args.out + ".png")

    n_faces = sum(len(p) for p in model.parts)
    n_verts = obj.count("\nv ")
    print(f"wrote {args.out}.obj/.mtl/.png ({n_verts} verts, {n_faces} faces)")


if __name__ == "__main__":
    main()
