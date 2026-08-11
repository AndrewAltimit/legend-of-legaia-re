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
                pp = int(span * (i + 0.5) / n)
                if horiz:
                    d.line([x0, y0 + pp, x1 - 1, y0 + pp], fill=streaks + (255,))
                else:
                    d.line([x0 + pp, y0, x0 + pp, y1 - 1], fill=streaks + (255,))

    # --- face tile (0,0)-(64,64): skin, red eye, dark eyepatch, mouth mask.
    skin = (236, 205, 184)
    patch(0, 0, 64, 64, skin)
    # her visible eye - big red iris
    d.rectangle([10, 22, 27, 37], fill=(250, 248, 246, 255))  # sclera
    d.rectangle([14, 24, 23, 37], fill=(198, 30, 42, 255))  # iris
    d.rectangle([17, 29, 21, 35], fill=(64, 8, 12, 255))  # pupil
    d.line([8, 18, 29, 16], fill=(88, 78, 84, 255), width=2)  # brow
    # the other eye: DARK grey eyepatch with a strap line (goal look)
    d.rectangle([38, 19, 57, 38], fill=(84, 80, 94, 255))
    d.rectangle([40, 21, 55, 36], fill=(60, 56, 70, 255))
    d.line([36, 14, 60, 22], fill=(38, 36, 46, 255), width=2)  # strap
    # mouth mask band with stitch ticks
    d.rectangle([4, 44, 60, 63], fill=(243, 241, 236, 255))
    for x in range(9, 58, 8):
        d.line([x, 48, x, 59], fill=(212, 207, 198, 255))

    # --- plain skin (64,0)-(96,32)
    patch(64, 0, 96, 32, skin)

    # --- hair silver (128,0)-(192,64): vertical strand streaks
    patch(128, 0, 192, 64, (208, 210, 220), streaks=(186, 189, 202), horiz=True, n=5)
    d.line([128, 20, 191, 20], fill=(134, 138, 152, 255))
    d.line([128, 44, 191, 44], fill=(134, 138, 152, 255))

    # --- hair dark (192,0)-(256,64): near-black (ribbons, tail tips)
    patch(192, 0, 256, 64, (40, 40, 50), streaks=(24, 24, 32), n=8)

    # --- tail two-tone (192,96)-(256,160): dark strands w/ silver lights
    patch(192, 96, 256, 160, (58, 58, 70), streaks=(30, 30, 40), horiz=True, n=10)
    for y in (110, 142):
        d.line([192, y, 255, y], fill=(140, 144, 158, 255))

    # --- dress black (0,64)-(64,128): soft fold lines
    patch(0, 64, 64, 128, (34, 32, 40), streaks=(22, 20, 26), n=5)

    # --- bandage white (64,64)-(128,128): horizontal wrap seams
    patch(64, 64, 128, 128, (238, 234, 226), streaks=(222, 216, 205), horiz=True, n=5)

    # --- glove black (128,64)-(160,96)
    patch(128, 64, 160, 96, (28, 26, 32))

    # --- rose red (160,64)-(192,96): petal swirl
    patch(160, 64, 192, 96, (152, 20, 34))
    d.ellipse([166, 70, 186, 90], outline=(96, 10, 20, 255), width=2)
    d.ellipse([172, 76, 180, 84], fill=(96, 10, 20, 255))

    # --- blade silver (224,64)-(256,96): stepped gradient + edge light
    for band in range(4):
        g = 150 + 30 * band
        d.rectangle([224 + band * 8, 64, 224 + band * 8 + 7, 95], fill=(g, g + 4, g + 10, 255))
    d.line([254, 64, 254, 95], fill=(255, 255, 255, 255))

    # --- arm band dark red (128,96)-(160,128)
    patch(128, 96, 160, 128, (112, 18, 30), streaks=(84, 12, 22), horiz=True, n=3)

    # --- boot / strap black (0,128)-(64,160)
    patch(0, 128, 64, 160, (26, 24, 30), streaks=(48, 44, 54), horiz=True, n=3)

    # --- stocking white (128,128)-(160,192): fine wrap
    patch(128, 128, 160, 192, (240, 236, 228), streaks=(226, 220, 209), horiz=True, n=5)

    return img


# UV patch centres (u0,v0,u1,v1) in texel space for each material region.
UV = {
    "face": (2, 2, 62, 62),
    "skin": (66, 2, 94, 30),
    "hair": (130, 2, 190, 62),
    "hair_dark": (194, 2, 254, 62),
    "hair_tail": (194, 98, 254, 158),
    "dress": (2, 66, 62, 126),
    "bandage": (66, 66, 126, 126),
    "glove": (130, 66, 158, 94),
    "rose": (162, 66, 190, 94),
    "blade": (226, 66, 254, 94),
    "band": (130, 98, 158, 126),
    "boot": (2, 130, 62, 158),
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
    m.prism(0, 0, -2, -70, -12, (27, 27), (20, 22), "hair", sides=8, cap_bot=False)
    # face plate over the front lower half
    m.face(
        0,
        [(-21, -54, 26), (21, -54, 26), (17, -14, 23), (-17, -14, 23)],
        "face",
        shade=[1.05, 1.05, 0.95, 0.95],
        uv_rect=[(2, 2), (62, 2), (62, 62), (2, 62)],
    )
    # bangs over the brow + long side locks framing the face
    for x0, x1, drop in ((-23, -8, 15), (-7, 7, 10), (8, 23, 15)):
        m.face(
            0,
            [(x0, -62, 28), (x1, -62, 28), (x1, -62 + drop, 31), (x0, -62 + drop, 31)],
            "hair",
            shade=[1.1, 1.1, 0.9, 0.9],
        )
    for sx in (-1, 1):
        m.face(
            0,
            [(sx * 26, -58, 22), (sx * 21, -58, 27), (sx * 17, -6, 22), (sx * 24, -8, 16)],
            "hair",
            shade=[1.05, 1.05, 0.8, 0.8],
        )
    # neck
    m.prism(0, 0, 0, -16, 4, (7, 7), (8, 8), "skin", sides=6, cap_top=False, cap_bot=False)
    # twintail ribbons (black), high on the sides
    for sx in (-1, 1):
        m.box(0, sx * 27 - 6, sx * 27 + 6, -66, -54, -10, 2, "hair_dark")
    # twintails: silver poof at the ribbon, then LONG dark two-tone strands
    # sweeping past the waist (the goal's tails reach the skirt)
    for sx in (-1, 1):
        m.prism(0, sx * 38, -6, -62, -8, (15, 17), (18, 20), "hair", sides=5, cap_top=True, cap_bot=False)
        m.prism(0, sx * 49, -10, -6, 54, (17, 19), (9, 10), "hair_tail", sides=5, cap_top=False, cap_bot=False)
        m.prism(0, sx * 53, -16, 54, 102, (9, 10), (2, 2), "hair_tail", sides=4, cap_top=False)

    # ----- part 1: torso (origin waist; up = -y) --------------------------
    # bare shoulders / upper chest (skin!), then the white chest wrap, then
    # a black bodice tapering to a narrow waist - the goal's figure
    m.prism(1, 0, -1, -98, -74, (21, 13), (23, 15), "skin", sides=8, cap_bot=False, mats_cap="skin")
    m.prism(1, 0, -1, -74, -54, (23, 15), (19, 12), "bandage", sides=8, cap_top=False, cap_bot=False)
    m.prism(1, 0, 0, -54, 2, (19, 12), (13, 9), "dress", sides=8, cap_top=False, cap_bot=False)
    # rose brooch at the chest, sitting on the wrap/bodice seam
    m.box(1, -6, 7, -60, -48, 12, 18, "rose", skip=("-z",))

    # ----- part 2: skirt (origin waist; down = +y) ------------------------
    # snug white bandage-wrap miniskirt...
    m.prism(2, 0, 0, -2, 32, (14, 10), (20, 15), "bandage", sides=8, cap_top=False, cap_bot=True)
    # ...under a long BLACK open-front train (back + sides only)
    sides = 8
    for i in range(sides):
        j = i + 1
        a0 = 2 * math.pi * (i + 0.5) / sides
        a1 = 2 * math.pi * (j + 0.5) / sides

        def ring_pt(ang, y, rx, rz):
            return (rx * math.cos(ang), y, rz * math.sin(ang))

        zmid = math.sin((a0 + a1) / 2)
        if zmid > 0.35:
            continue  # open front - the white wrap shows through
        top0 = ring_pt(a0, -2, 17, 12)
        top1 = ring_pt(a1, -2, 17, 12)
        hem0 = ring_pt(a0, 104, 46, 40)
        hem1 = ring_pt(a1, 104, 46, 40)
        sd = 0.86 + 0.14 * zmid
        m.face(2, [top0, top1, hem1, hem0], "dress", shade=[sd * 1.05, sd * 1.05, sd * 0.7, sd * 0.7])
    # waist rose + falling ribbon at the front centre
    m.box(2, -6, 6, 0, 12, 13, 19, "rose", skip=("-z",))
    m.box(2, -2, 2, 12, 30, 14, 16, "band", skip=("-z",))

    # ----- part 3: LEFT upper arm (bare skin + dark-red arm band) ---------
    m.prism(3, 0, 0, -6, 56, (9, 9), (7, 7), "skin", sides=6, cap_top=True, cap_bot=False)
    m.prism(3, 0, 0, 12, 19, (9.6, 9.6), (9.6, 9.6), "band", sides=6, cap_top=False, cap_bot=False)

    # ----- part 4: LEFT forearm (white bandage wrap) ----------------------
    m.prism(4, 0, 0, -3, 56, (9, 9), (7, 7), "bandage", sides=6, cap_top=True, cap_bot=False)

    # ----- part 5: LEFT hand (black glove + a red rose) -------------------
    m.box(5, -8, 8, 0, 26, -7, 9, "glove")
    m.box(5, -6, 6, 24, 33, -5, 7, "glove")  # fingers
    m.box(5, -7, 7, 6, 22, 9, 21, "rose")  # clutched rose

    # ----- part 6/7: RIGHT arm = mirror of left ---------------------------
    mirror_part(m, 3, 6)
    mirror_part(m, 4, 7)

    # ----- part 8: RIGHT hand (glove + dagger) ----------------------------
    m.box(8, -8, 8, 0, 26, -7, 9, "glove")
    m.box(8, -6, 6, 24, 33, -5, 7, "glove")
    m.box(8, -4, 4, 31, 37, -14, 16, "glove")  # crossguard
    m.box(8, -2, 2, 37, 74, -4, 12, "blade")
    m.face(8, [(-2, 74, 12), (2, 74, 12), (0, 88, 4)], "blade", shade=[1.1, 1.1, 1.2])

    # ----- part 9: LEFT thigh (skin gap, garter, white wrap) --------------
    m.prism(9, 0, 0, -6, 24, (13, 12), (13, 12), "skin", sides=6, cap_top=True, cap_bot=False)
    m.prism(9, 0, 0, 24, 31, (14, 13), (14, 13), "boot", sides=6, cap_top=False, cap_bot=False)
    m.prism(9, 0, 0, 31, 100, (13, 12), (10, 10), "stocking", sides=6, cap_top=False, cap_bot=False)

    # ----- part 10: LEFT shin (white wrap boot + black straps) ------------
    m.prism(10, 0, 0, -4, 118, (11, 11), (9, 9), "stocking", sides=6, cap_top=True, cap_bot=False)
    m.prism(10, 0, 0, 34, 41, (11.3, 11.3), (10.8, 10.8), "boot", sides=6, cap_top=False, cap_bot=False)

    # ----- part 11: LEFT foot (small black shoe) --------------------------
    m.box(11, -8, 8, 0, 22, -14, 28, "boot")
    m.box(11, -6, 6, 12, 22, 28, 44, "boot")  # toe

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
