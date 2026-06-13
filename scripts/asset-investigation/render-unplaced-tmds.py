#!/usr/bin/env python3
"""Render wireframe contact sheets for each kingdom's unplaced slot-1 TMDs.

For each kingdom in `site/world-overview.json`, decompresses the slot-1 TMD
pack, slices each unplaced TMD body, parses it via `target/release/tmd
dump-obj`, and rasterises a 3-view wireframe (top / front / iso). Composites
the thumbnails into one grid PNG per kingdom under `--out-dir`.

Used for the visual classification sweep that populates
`site/world-overview/slot1_classification.toml`.

    python3 scripts/asset-investigation/render-unplaced-tmds.py \\
        --prot-dir /tmp/legaia-extract/PROT \\
        --out-dir /tmp/world-thumbs
"""
from __future__ import annotations
import argparse
import json
import math
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

KINGDOMS = [
    (85,  "drake",   "Drake Kingdom"),
    (244, "sebucus", "Sebucus Islands"),
    (391, "karisto", "Karisto Kingdom"),
]


def find_asset_table(buf: bytes) -> int | None:
    for off in range(0, len(buf), 0x800):
        if off + 64 > len(buf):
            break
        if struct.unpack_from("<I", buf, off)[0] != 7:
            continue
        if struct.unpack_from("<I", buf, off + 12)[0] == 0x40:
            return off
    return None


def lzs_decompress(lzs_bin: Path, src: bytes, decompressed_size: int) -> bytes:
    with tempfile.NamedTemporaryFile(delete=False) as src_f:
        src_f.write(src)
        src_path = src_f.name
    with tempfile.NamedTemporaryFile(delete=False) as dst_f:
        dst_path = dst_f.name
    try:
        subprocess.run(
            [str(lzs_bin), "raw", src_path, "--size", str(decompressed_size),
             "--output", dst_path],
            capture_output=True, text=True, check=True,
        )
        return Path(dst_path).read_bytes()
    finally:
        Path(src_path).unlink(missing_ok=True)
        Path(dst_path).unlink(missing_ok=True)


def decompress_slot1_pack(prot_path: Path, lzs_bin: Path) -> bytes:
    buf = prot_path.read_bytes()
    table = find_asset_table(buf)
    if table is None:
        raise RuntimeError(f"no 7-asset table in {prot_path}")
    type_size = struct.unpack_from("<I", buf, table + 8 + 1 * 8)[0]
    offset = struct.unpack_from("<I", buf, table + 8 + 1 * 8 + 4)[0]
    type_byte = type_size >> 24
    size = type_size & 0xFF_FF_FF
    if type_byte != 0x02:
        raise RuntimeError(f"slot 1 type 0x{type_byte:02X} != 0x02 in {prot_path}")
    return lzs_decompress(lzs_bin, buf[table + offset:], size)


def parse_obj(obj_text: str) -> tuple[list[tuple[float, float, float]], list[list[int]]]:
    """Returns (vertices, faces). Vertex indices in faces are 0-based."""
    verts: list[tuple[float, float, float]] = []
    faces: list[list[int]] = []
    for line in obj_text.splitlines():
        if line.startswith("v "):
            parts = line.split()
            verts.append((float(parts[1]), float(parts[2]), float(parts[3])))
        elif line.startswith("f "):
            parts = line.split()[1:]
            face = [int(p.split("/")[0]) - 1 for p in parts]
            if len(face) >= 3:
                faces.append(face)
    return verts, faces


def rotate_yaw_pitch(v: tuple[float, float, float], yaw: float, pitch: float) -> tuple[float, float, float]:
    cy, sy = math.cos(yaw), math.sin(yaw)
    cp, sp = math.cos(pitch), math.sin(pitch)
    x, y, z = v
    x2 = cy * x + sy * z
    z2 = -sy * x + cy * z
    y3 = cp * y - sp * z2
    z3 = sp * y + cp * z2
    return (x2, y3, z3)


def render_view(
    verts: list[tuple[float, float, float]],
    faces: list[list[int]],
    size: int,
    yaw_deg: float,
    pitch_deg: float,
    bg=(20, 22, 26, 255),
    fg=(180, 200, 230, 255),
    accent=(255, 220, 120, 255),
) -> Image.Image:
    """Wireframe orthographic render. Y-up assumed."""
    img = Image.new("RGBA", (size, size), bg)
    if not verts or not faces:
        return img
    yaw = math.radians(yaw_deg)
    pitch = math.radians(pitch_deg)
    # PSX convention: Y points DOWN (positive Y = below ground). Flip so up is up.
    rot = [rotate_yaw_pitch((v[0], -v[1], v[2]), yaw, pitch) for v in verts]
    xs = [v[0] for v in rot]
    ys = [v[1] for v in rot]
    if not xs:
        return img
    cx = (max(xs) + min(xs)) / 2
    cy = (max(ys) + min(ys)) / 2
    span = max(max(xs) - min(xs), max(ys) - min(ys), 1.0)
    scale = (size * 0.85) / span
    def proj(v):
        sx = (v[0] - cx) * scale + size / 2
        sy = -((v[1] - cy) * scale) + size / 2
        return (sx, sy)
    screen = [proj(v) for v in rot]
    draw = ImageDraw.Draw(img)
    # Draw face edges. For quads draw 4 edges, for tris 3.
    for face in faces:
        n = len(face)
        for i in range(n):
            a = face[i]
            b = face[(i + 1) % n]
            if a >= len(screen) or b >= len(screen):
                continue
            draw.line([screen[a], screen[b]], fill=fg, width=1)
    # Accent: bbox in projected space (subtle frame).
    sxs = [p[0] for p in screen]
    sys_ = [p[1] for p in screen]
    if sxs:
        draw.rectangle([min(sxs), min(sys_), max(sxs), max(sys_)],
                       outline=(60, 70, 80, 255), width=1)
    return img


CLASS_TINT = {
    "landmark":    (255, 220, 120, 255),
    "ground_tile": (140, 200, 255, 255),
    "decoration":  (200, 150, 255, 255),
    "npc_token":   (255, 160, 180, 255),
    "unknown":     (200, 200, 210, 255),
}


def render_tmd_cell(
    obj_text: str,
    cell_w: int,
    cell_h: int,
    label: str,
    subtitle: str,
    cls: str,
    font: ImageFont.ImageFont,
    small_font: ImageFont.ImageFont,
) -> Image.Image:
    """Big iso view + smaller top/front stacked on the right, plus a 2-line
    label band at the bottom tinted by current class."""
    verts, faces = parse_obj(obj_text)
    pad = 4
    text_band = 30
    view_h = cell_h - text_band - 2 * pad
    iso_size = view_h
    side_size = (view_h - pad) // 2
    iso_color = CLASS_TINT.get(cls, CLASS_TINT["unknown"])
    cell = Image.new("RGBA", (cell_w, cell_h), (12, 14, 18, 255))
    iso = render_view(verts, faces, iso_size, yaw_deg=35.0, pitch_deg=25.0,
                      fg=iso_color)
    top = render_view(verts, faces, side_size, yaw_deg=0.0, pitch_deg=90.0,
                      fg=(150, 220, 180, 255))
    front = render_view(verts, faces, side_size, yaw_deg=0.0, pitch_deg=0.0,
                        fg=(220, 180, 150, 255))
    cell.paste(iso, (pad, pad))
    cell.paste(top, (pad + iso_size + pad, pad))
    cell.paste(front, (pad + iso_size + pad, pad + side_size + pad))
    draw = ImageDraw.Draw(cell)
    band_y = pad + view_h + pad
    # Class swatch bar (left edge of band).
    draw.rectangle([0, band_y, 6, cell_h], fill=iso_color)
    draw.text((10, band_y + 1), label, fill=(240, 240, 240, 255), font=font)
    draw.text((10, band_y + 15), subtitle, fill=(180, 180, 190, 255),
              font=small_font)
    # Tiny view labels (top-left of each view).
    draw.text((pad + 3, pad + 2), "iso", fill=(80, 80, 90, 255), font=small_font)
    draw.text((pad + iso_size + pad + 3, pad + 2), "top",
              fill=(80, 80, 90, 255), font=small_font)
    draw.text((pad + iso_size + pad + 3, pad + side_size + pad + 2), "front",
              fill=(80, 80, 90, 255), font=small_font)
    return cell


def get_default_font() -> tuple[ImageFont.ImageFont, ImageFont.ImageFont]:
    """Return (regular, small) fonts. Falls back to PIL default if no TTF."""
    candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf",
    ]
    for path in candidates:
        if Path(path).exists():
            try:
                return ImageFont.truetype(path, 13), ImageFont.truetype(path, 9)
            except Exception:
                continue
    return ImageFont.load_default(), ImageFont.load_default()


def render_kingdom_grid(
    kingdom_key: str,
    kingdom_label: str,
    pack_bytes: bytes,
    unplaced: list[dict],
    out_path: Path,
    tmd_bin: Path,
    cols: int = 4,
    cell_w: int = 320,
    cell_h: int = 220,
    rows_per_page: int | None = None,
) -> None:
    if not unplaced:
        print(f"  {kingdom_key}: no unplaced slots")
        return
    margin = 14
    header_h = 44
    pages: list[list[dict]] = []
    if rows_per_page is None:
        pages = [unplaced]
    else:
        per_page = cols * rows_per_page
        for i in range(0, len(unplaced), per_page):
            pages.append(unplaced[i:i + per_page])
    font, small = get_default_font()
    for page_idx, page in enumerate(pages):
        rows = math.ceil(len(page) / cols)
        grid_w = cols * cell_w + (cols + 1) * margin
        grid_h = header_h + rows * cell_h + (rows + 1) * margin
        grid = Image.new("RGBA", (grid_w, grid_h), (8, 9, 12, 255))
        draw = ImageDraw.Draw(grid)
        suffix = f" (page {page_idx + 1}/{len(pages)})" if len(pages) > 1 else ""
        draw.text((margin, 12),
                  f"{kingdom_label} - {len(unplaced)} unplaced slot-1 TMDs{suffix}",
                  fill=(230, 240, 250, 255), font=font)
        draw.text((margin, 28),
                  "Per cell: iso (yellow=landmark / blue=ground_tile / white=unknown) | top | front",
                  fill=(140, 150, 170, 255), font=small)
        for i, u in enumerate(page):
            cell = build_cell(u, pack_bytes, tmd_bin, cell_w, cell_h, font, small)
            col = i % cols
            row = i // cols
            x = margin + col * (cell_w + margin)
            y = header_h + margin + row * (cell_h + margin)
            grid.paste(cell, (x, y))
        if len(pages) > 1:
            page_path = out_path.with_name(f"{out_path.stem}_p{page_idx + 1}{out_path.suffix}")
        else:
            page_path = out_path
        grid.save(page_path)
        print(f"  {kingdom_key}: wrote {page_path} ({page_path.stat().st_size:,} bytes, "
              f"{len(page)} cells)")


def build_cell(u, pack_bytes, tmd_bin, cell_w, cell_h, font, small):
    slot = u["pack_slot"]
    body = pack_bytes[u["byte_offset"]:u["byte_end"]]
    with tempfile.NamedTemporaryFile(suffix=".tmd", delete=False) as tf:
        tf.write(body)
        tmd_path = tf.name
    with tempfile.NamedTemporaryFile(suffix=".obj", delete=False) as of:
        obj_path = of.name
    try:
        r = subprocess.run([str(tmd_bin), "dump-obj", tmd_path, "-o", obj_path],
                           capture_output=True, text=True)
        if r.returncode != 0:
            cell = Image.new("RGBA", (cell_w, cell_h), (40, 18, 18, 255))
            cd = ImageDraw.Draw(cell)
            cd.text((6, 6), f"slot {slot}: parse fail",
                    fill=(230, 180, 180, 255), font=font)
            cd.text((6, 22), (r.stderr or "no stderr")[:80],
                    fill=(200, 160, 160, 255), font=small)
            return cell
        obj_text = Path(obj_path).read_text()
        label = f"slot {slot:>3}  nobj={u['nobj']}  {u['body_bytes']}B"
        subtitle = f"md5 {u['md5']}  class={u['class']}"
        return render_tmd_cell(obj_text, cell_w, cell_h, label, subtitle,
                               u["class"], font, small)
    finally:
        Path(tmd_path).unlink(missing_ok=True)
        Path(obj_path).unlink(missing_ok=True)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--prot-dir", default="/tmp/legaia-extract/PROT")
    ap.add_argument("--lzs-bin", default="target/release/lzs-decode")
    ap.add_argument("--tmd-bin", default="target/release/tmd")
    ap.add_argument("--overview", default="site/world-overview.json")
    ap.add_argument("--out-dir", default="/tmp/world-thumbs")
    ap.add_argument("--cols", type=int, default=3,
                    help="Columns per grid page. Default: %(default)s")
    ap.add_argument("--cell-w", type=int, default=480,
                    help="Cell width in px. Default: %(default)s")
    ap.add_argument("--cell-h", type=int, default=300,
                    help="Cell height in px. Default: %(default)s")
    ap.add_argument("--rows-per-page", type=int, default=4,
                    help="Rows per grid page (0 = single page). "
                         "Pages are written with `_pN` suffix. Default: %(default)s")
    args = ap.parse_args()
    prot_dir = Path(args.prot_dir)
    lzs_bin = Path(args.lzs_bin)
    tmd_bin = Path(args.tmd_bin)
    for tool in (lzs_bin, tmd_bin):
        if not tool.exists():
            sys.exit(f"missing tool: {tool}. cargo build --release first.")
    overview = json.loads(Path(args.overview).read_text())
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    for base, key, label in KINGDOMS:
        matches = sorted(prot_dir.glob(f"{base:04d}_*.BIN"))
        if not matches:
            print(f"  {key}: PROT {base} missing under {prot_dir}; skipping",
                  file=sys.stderr)
            continue
        pack = decompress_slot1_pack(matches[0], lzs_bin)
        unplaced = overview[key]["unplaced_slot1_tmds"]
        out_path = out_dir / f"{key}_unplaced.png"
        render_kingdom_grid(
            key, label, pack, unplaced, out_path, tmd_bin,
            cols=args.cols, cell_w=args.cell_w, cell_h=args.cell_h,
            rows_per_page=args.rows_per_page if args.rows_per_page > 0 else None,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
