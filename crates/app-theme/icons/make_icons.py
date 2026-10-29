#!/usr/bin/env python3
"""Draw the desktop programs' icons and write every committed icon file.

The source of truth for both icons is the shape list in this script: each
icon is a handful of flat shapes on a 1024 x 1024 canvas. The script writes
the same shapes twice - as an SVG (`<name>.svg`, for editing and the web) and
rasterised with Pillow (no SVG renderer is needed) into:

    <name>.png    256 x 256, the runtime window icon (include_bytes! in lib.rs)
    <name>.ico    16..256 px, embedded into the Windows .exe (win_icon.rs)
    <name>.icns   16..1024 px, the macOS .app bundle icon (release-build.sh)

Both icons are original artwork for this project: no logo, box art or game
asset is drawn from or traced. Re-run after editing a shape list:

    python3 crates/app-theme/icons/make_icons.py

Needs Pillow. Output is deterministic for a given Pillow version.
"""

import math
import os
import sys

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
SIZE = 1024
SUPER = 4  # supersampling factor for the master raster
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

# The background tile every icon sits on: a rounded square.
TILE = (64, 64, 960, 960, 200)


def rgba(hex_or_tuple, alpha=255):
    if isinstance(hex_or_tuple, tuple):
        return hex_or_tuple
    h = hex_or_tuple.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), alpha)


def vgrad(y0, y1, top, bottom):
    """A vertical linear gradient fill from y0 (top colour) to y1."""
    return ("vgrad", y0, y1, rgba(top), rgba(bottom))


def pie(cx, cy, r, a0, a1, steps=48):
    """A pie slice as a polygon, angles in degrees (0 = +x, clockwise)."""
    pts = [(cx, cy)]
    for i in range(steps + 1):
        a = math.radians(a0 + (a1 - a0) * i / steps)
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


# --- The two icons ---------------------------------------------------------
#
# Shape tuples: (kind, geometry..., fill, clip). `clip` keeps the shape
# inside the background tile.

GOLD = "#f2c14e"
GOLD_DARK = "#c8902f"

# Legend of Legaia: a golden tree under a night sky, mist drifting across
# its roots.
GAME = [
    ("rrect", TILE, vgrad(64, 960, "#1d2b53", "#2a6f73"), False),
    ("circle", (512, 400, 330), rgba((255, 214, 140, 26)), True),
    ("circle", (512, 400, 250), rgba((255, 214, 140, 34)), True),
    ("ellipse", (512, 930, 470, 150), rgba("#14203a"), True),
    # trunk with a root flare
    ("poly", [(452, 840), (480, 760), (494, 560), (530, 560), (544, 760),
              (572, 840)], rgba("#7a4e24"), False),
    # canopy: a shadow pass, then the gold crown
    ("circle", (370, 462, 130), rgba(GOLD_DARK), False),
    ("circle", (654, 462, 130), rgba(GOLD_DARK), False),
    ("circle", (512, 400, 190), rgba(GOLD_DARK), False),
    ("circle", (370, 448, 126), rgba(GOLD), False),
    ("circle", (654, 448, 126), rgba(GOLD), False),
    ("circle", (512, 382, 186), rgba(GOLD), False),
    ("circle", (432, 312, 118), rgba(GOLD), False),
    ("circle", (592, 312, 118), rgba(GOLD), False),
    ("circle", (470, 270, 52), rgba((255, 236, 170, 120)), False),
    # mist
    ("ellipse", (310, 712, 250, 34), rgba((225, 238, 250, 110)), True),
    ("ellipse", (720, 770, 270, 32), rgba((225, 238, 250, 100)), True),
    ("ellipse", (470, 840, 330, 34), rgba((225, 238, 250, 120)), True),
]

# Legaia Explorer: a disc under a magnifying glass.
DISC_C = (440, 440)
EXPLORER = [
    ("rrect", TILE, vgrad(64, 960, "#2b3a4d", "#121821"), False),
    ("circle", (DISC_C[0], DISC_C[1], 320), vgrad(120, 760, "#eef2f7", "#93a1b4"), False),
    ("poly", pie(DISC_C[0], DISC_C[1], 318, 200, 245), rgba((140, 200, 255, 80)), False),
    ("poly", pie(DISC_C[0], DISC_C[1], 318, 20, 65), rgba((255, 170, 220, 70)), False),
    ("ring", (DISC_C[0], DISC_C[1], 250, 6), rgba((255, 255, 255, 90)), False),
    ("ring", (DISC_C[0], DISC_C[1], 180, 6), rgba((90, 105, 125, 90)), False),
    ("circle", (DISC_C[0], DISC_C[1], 96), rgba("#d3dae4"), False),
    ("ring", (DISC_C[0], DISC_C[1], 96, 8), rgba("#5f6c7d"), False),
    ("circle", (DISC_C[0], DISC_C[1], 40), rgba("#1c2633"), False),
    # magnifier
    ("line", (780, 780, 892, 892, 84), rgba(GOLD_DARK), False),
    ("line", (770, 770, 830, 830, 84), rgba(GOLD), False),
    ("circle", (650, 650, 170), rgba((175, 215, 255, 95)), False),
    ("ring", (650, 650, 170, 46), rgba(GOLD), False),
    ("circle", (596, 590, 46), rgba((255, 255, 255, 110)), False),
]

ICONS = {"legend-of-legaia": GAME, "legaia-explorer": EXPLORER}


# --- SVG -------------------------------------------------------------------

def svg_color(c):
    r, g, b, a = c
    s = 'fill="#%02x%02x%02x"' % (r, g, b)
    if a != 255:
        s += ' fill-opacity="%.3f"' % (a / 255)
    return s


def svg_stroke(c):
    r, g, b, a = c
    s = 'stroke="#%02x%02x%02x"' % (r, g, b)
    if a != 255:
        s += ' stroke-opacity="%.3f"' % (a / 255)
    return s


def to_svg(shapes):
    defs, body = [], []
    x0, y0, x1, y1, r = TILE
    defs.append('<clipPath id="tile"><rect x="%d" y="%d" width="%d" height="%d" '
                'rx="%d"/></clipPath>' % (x0, y0, x1 - x0, y1 - y0, r))
    for i, (kind, geo, fill, clip) in enumerate(shapes):
        if fill[0] == "vgrad":
            _, gy0, gy1, top, bottom = fill
            gid = "g%d" % i
            defs.append(
                '<linearGradient id="%s" gradientUnits="userSpaceOnUse" x1="0" y1="%d" '
                'x2="0" y2="%d"><stop offset="0" stop-color="#%02x%02x%02x"/>'
                '<stop offset="1" stop-color="#%02x%02x%02x"/></linearGradient>'
                % (gid, gy0, gy1, top[0], top[1], top[2], bottom[0], bottom[1], bottom[2]))
            paint = 'fill="url(#%s)"' % gid
        else:
            paint = svg_color(fill)
        cp = ' clip-path="url(#tile)"' if clip else ""
        if kind == "rrect":
            a, b, c, d, rr = geo
            body.append('<rect x="%d" y="%d" width="%d" height="%d" rx="%d" %s%s/>'
                        % (a, b, c - a, d - b, rr, paint, cp))
        elif kind == "circle":
            cx, cy, rr = geo
            body.append('<circle cx="%d" cy="%d" r="%d" %s%s/>' % (cx, cy, rr, paint, cp))
        elif kind == "ellipse":
            cx, cy, rx, ry = geo
            body.append('<ellipse cx="%d" cy="%d" rx="%d" ry="%d" %s%s/>'
                        % (cx, cy, rx, ry, paint, cp))
        elif kind == "poly":
            pts = " ".join("%.1f,%.1f" % p for p in geo)
            body.append('<polygon points="%s" %s%s/>' % (pts, paint, cp))
        elif kind == "ring":
            cx, cy, rr, w = geo
            body.append('<circle cx="%d" cy="%d" r="%d" fill="none" stroke-width="%d" %s%s/>'
                        % (cx, cy, rr, w, svg_stroke(fill), cp))
        elif kind == "line":
            a, b, c, d, w = geo
            body.append('<line x1="%d" y1="%d" x2="%d" y2="%d" stroke-width="%d" '
                        'stroke-linecap="round" %s%s/>' % (a, b, c, d, w, svg_stroke(fill), cp))
        else:
            raise ValueError(kind)
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d">\n'
            '<defs>\n%s\n</defs>\n%s\n</svg>\n'
            % (SIZE, SIZE, SIZE, SIZE, "\n".join(defs), "\n".join(body)))


# --- Raster ----------------------------------------------------------------

def shape_mask(kind, geo, n):
    """The shape's coverage at n x n (coordinates scaled from SIZE)."""
    k = n / SIZE
    m = Image.new("L", (n, n), 0)
    d = ImageDraw.Draw(m)
    if kind == "rrect":
        a, b, c, e, r = geo
        d.rounded_rectangle([a * k, b * k, c * k, e * k], radius=r * k, fill=255)
    elif kind == "circle":
        cx, cy, r = geo
        d.ellipse([(cx - r) * k, (cy - r) * k, (cx + r) * k, (cy + r) * k], fill=255)
    elif kind == "ellipse":
        cx, cy, rx, ry = geo
        d.ellipse([(cx - rx) * k, (cy - ry) * k, (cx + rx) * k, (cy + ry) * k], fill=255)
    elif kind == "poly":
        d.polygon([(x * k, y * k) for x, y in geo], fill=255)
    elif kind == "ring":
        cx, cy, r, w = geo
        ro, ri = (r + w / 2) * k, (r - w / 2) * k
        d.ellipse([cx * k - ro, cy * k - ro, cx * k + ro, cy * k + ro], fill=255)
        d.ellipse([cx * k - ri, cy * k - ri, cx * k + ri, cy * k + ri], fill=0)
    elif kind == "line":
        a, b, c, e, w = geo
        d.line([(a * k, b * k), (c * k, e * k)], fill=255, width=int(round(w * k)))
        rr = w * k / 2
        for x, y in ((a, b), (c, e)):
            d.ellipse([x * k - rr, y * k - rr, x * k + rr, y * k + rr], fill=255)
    else:
        raise ValueError(kind)
    return m


def paint_image(fill, n):
    if fill[0] == "vgrad":
        _, y0, y1, top, bottom = fill
        col = Image.new("RGBA", (1, n))
        for y in range(n):
            t = min(1.0, max(0.0, (y * SIZE / n - y0) / float(y1 - y0)))
            col.putpixel((0, y), tuple(int(round(top[i] + (bottom[i] - top[i]) * t))
                                       for i in range(4)))
        return col.resize((n, n), Image.NEAREST)
    return Image.new("RGBA", (n, n), fill)


def render(shapes):
    n = SIZE * SUPER
    canvas = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    clip = shape_mask("rrect", TILE, n)
    for kind, geo, fill, clipped in shapes:
        mask = shape_mask(kind, geo, n)
        if clipped:
            mask = Image.composite(mask, Image.new("L", (n, n), 0), clip)
        layer = paint_image(fill, n)
        alpha = Image.composite(layer.getchannel("A"), Image.new("L", (n, n), 0), mask)
        layer.putalpha(alpha)
        canvas.alpha_composite(layer)
    return canvas.resize((SIZE, SIZE), Image.LANCZOS)


def main():
    for name, shapes in ICONS.items():
        with open(os.path.join(HERE, name + ".svg"), "w") as f:
            f.write(to_svg(shapes))
        master = render(shapes)
        master.resize((256, 256), Image.LANCZOS).save(
            os.path.join(HERE, name + ".png"), optimize=True)
        master.save(os.path.join(HERE, name + ".ico"),
                    sizes=[(s, s) for s in ICO_SIZES])
        master.save(os.path.join(HERE, name + ".icns"))
        print("wrote %s.{svg,png,ico,icns}" % name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
