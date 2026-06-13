#!/usr/bin/env python3
"""
slot4_topdown_png.py

Render a top-down (XZ-plane) image of slot 4's decoded geometry,
treating each `count_a`-record group as a polygon. Saves as a PGM
greyscale image so the result can be opened with any image viewer
without external deps.

If body 4-style 'normal/vertex pair' alternation is present
(--strip-zero-records), the zero records are dropped before rendering.
"""

import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "pcsx-redux"))
from match_prim_groups_to_disc import find_asset_table, lzs_decompress  # noqa: E402


KINGDOM_BASE = {"map01": 85, "map02": 244, "map03": 391}


def load_slot4(extracted_dir: Path, bundle: str):
    base = KINGDOM_BASE[bundle]
    for off in (0, 1):
        idx = base + off
        files = list(extracted_dir.joinpath("PROT").glob(f"{idx:04d}_*.BIN"))
        if not files:
            continue
        raw = files[0].read_bytes()
        table_off = find_asset_table(raw)
        if table_off is None:
            continue
        table = raw[table_off:]
        ts = struct.unpack("<I", table[40:44])[0]
        do = struct.unpack("<I", table[44:48])[0]
        slot_size = ts & 0xFFFFFF
        try:
            decoded = lzs_decompress(table[do:], slot_size)
        except Exception:
            continue
        return files[0].name, decoded
    return None, None


def collect_groups(decoded):
    """Yield (body_idx, group_idx, [(x,y,z,attr), ...]) for every group."""
    count = struct.unpack("<I", decoded[0:4])[0]
    byte_offsets = [
        struct.unpack("<I", decoded[4 + 4 * k : 8 + 4 * k])[0]
        for k in range(count)
    ]
    for k in range(count):
        s = byte_offsets[k]
        e = byte_offsets[k + 1] if k + 1 < count else len(decoded)
        body = decoded[s:e]
        if len(body) < 8:
            continue
        ca, _fa, cb, _fb = body[0], body[1], body[2], body[3]
        n_records = ca * cb
        if 8 + n_records * 8 + 8 > len(body):
            continue
        for g in range(cb):
            grp = []
            for v in range(ca):
                off = 8 + (g * ca + v) * 8
                x, y, z, a = struct.unpack("<4h", body[off : off + 8])
                grp.append((x, y, z, a))
            yield k, g, grp


def draw_line(img, w, h, x0, y0, x1, y1, color):
    """Bresenham line drawer that writes `color` into img[y*w + x]."""
    dx = abs(x1 - x0)
    dy = -abs(y1 - y0)
    sx = 1 if x0 < x1 else -1
    sy = 1 if y0 < y1 else -1
    err = dx + dy
    while True:
        if 0 <= x0 < w and 0 <= y0 < h:
            img[y0 * w + x0] = color
        if x0 == x1 and y0 == y1:
            break
        e2 = 2 * err
        if e2 >= dy:
            err += dy
            x0 += sx
        if e2 <= dx:
            err += dx
            y0 += sy


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--bundle", default="map01", choices=sorted(KINGDOM_BASE.keys()))
    ap.add_argument("--extracted", default="extracted")
    ap.add_argument("--out", default="/tmp/slot4_topdown.pgm")
    ap.add_argument("--size", type=int, default=1024)
    ap.add_argument(
        "--bounds",
        type=int,
        default=33000,
        help="World coord half-extent mapped to image edge.",
    )
    ap.add_argument(
        "--strip-zero-records",
        action="store_true",
        help="Drop records where (x,y,z)==(0,0,0).",
    )
    ap.add_argument(
        "--only-body",
        type=int,
        help="Render only one body's groups (default: all).",
    )
    ap.add_argument(
        "--points-only",
        action="store_true",
        help="Plot dots, no polygon connections.",
    )
    args = ap.parse_args()

    name, decoded = load_slot4(Path(args.extracted), args.bundle)
    if decoded is None:
        print(f"!! could not load slot 4 for {args.bundle}")
        return 1
    print(f"bundle: {name}")

    w = h = args.size
    img = bytearray(w * h)

    def to_pix(world_x, world_z):
        # world_x/z in roughly +/- bounds. Map to [0..w).
        # Z grows downward in image (screen Y).
        px = int((world_x + args.bounds) / (2 * args.bounds) * w)
        py = int((world_z + args.bounds) / (2 * args.bounds) * h)
        return px, py

    groups_drawn = 0
    for body_idx, group_idx, recs in collect_groups(decoded):
        if args.only_body is not None and body_idx != args.only_body:
            continue
        if args.strip_zero_records:
            recs = [r for r in recs if not (r[0] == 0 and r[1] == 0 and r[2] == 0)]
        if len(recs) < 1:
            continue
        # Body-dependent intensity
        intensity = 80 + (body_idx * 13) % 175
        if args.points_only:
            for x0, _, z0, _ in recs:
                px0, py0 = to_pix(x0, z0)
                if 0 <= px0 < w and 0 <= py0 < h:
                    img[py0 * w + px0] = max(img[py0 * w + px0], intensity)
                    # Make dots more visible
                    for dx in (-1, 0, 1):
                        for dy in (-1, 0, 1):
                            x, y = px0 + dx, py0 + dy
                            if 0 <= x < w and 0 <= y < h:
                                img[y * w + x] = max(img[y * w + x], intensity)
        else:
            if len(recs) < 2:
                continue
            # Draw closed polygon
            for v in range(len(recs)):
                x0, _, z0, _ = recs[v]
                x1, _, z1, _ = recs[(v + 1) % len(recs)]
                px0, py0 = to_pix(x0, z0)
                px1, py1 = to_pix(x1, z1)
                draw_line(img, w, h, px0, py0, px1, py1, intensity)
        groups_drawn += 1

    # Crosshair to mark origin
    cx, cy = w // 2, h // 2
    for d in range(-8, 9):
        if 0 <= cx + d < w:
            img[cy * w + cx + d] = max(img[cy * w + cx + d], 255)
        if 0 <= cy + d < h:
            img[(cy + d) * w + cx] = max(img[(cy + d) * w + cx], 255)

    # Write PGM P5
    out_path = Path(args.out)
    with out_path.open("wb") as f:
        f.write(f"P5\n{w} {h}\n255\n".encode("ascii"))
        f.write(bytes(img))
    print(f"wrote {out_path}  groups={groups_drawn}")

    # Also write a PNG via stdlib if possible (we just need the PGM
    # for inspection but a converter helps).
    try:
        import subprocess
        png_path = out_path.with_suffix(".png")
        subprocess.run(
            ["convert", str(out_path), str(png_path)],
            check=False,
            capture_output=True,
        )
        if png_path.exists():
            print(f"  ... converted to {png_path}")
    except Exception:
        pass


if __name__ == "__main__":
    sys.exit(main() or 0)
