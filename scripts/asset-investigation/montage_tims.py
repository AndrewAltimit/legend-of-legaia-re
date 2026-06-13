#!/usr/bin/env python3
"""Compose indexed contact sheets from `asset tim-render-distinct` output.

Drives the `tim_labels` visual-categorization pass: each distinct decoded TIM
gets a stable global index; sheets pack many cells (image + index + dims) so a
reviewer (or subagent) can categorize a whole batch from one image and report
`index -> label`. `index.tsv` maps every index back to its fingerprint.

The rendered PNGs are decoded Sony pixel data and are LOCAL ONLY (never
committed); only the resulting fingerprint->label table is committed.

Usage:
    python3 scripts/asset-investigation/montage_tims.py <render_dir> [--cols 7 --rows 7]

Inputs : <render_dir>/manifest.tsv + <render_dir>/<fnv>.png
Outputs: <render_dir>/sheets/sheet_NNN.png   (contact sheets)
         <render_dir>/index.tsv              (global_index <TAB> fnv1a <TAB> ...)
"""
import argparse
import os
import sys

from PIL import Image, ImageDraw, ImageFont

CELL_IMG = 100  # square image area per cell
LABEL_H = 22  # caption strip height
PAD = 4
BG = (10, 14, 21)
CELL_BG = (17, 22, 31)
IDX_BG = (40, 90, 140)
TEXT = (200, 210, 220)


def load_manifest(render_dir):
    rows = []
    with open(os.path.join(render_dir, "manifest.tsv")) as f:
        header = f.readline()
        del header
        for line in f:
            p = line.rstrip("\n").split("\t")
            if len(p) < 6:
                continue
            rows.append(
                {
                    "fnv": p[0],
                    "tier": p[1],
                    "w": int(p[2]),
                    "h": int(p[3]),
                    "bpp": int(p[4]),
                    "clut": int(p[5]),
                }
            )
    # Stable order by fingerprint, so global indices are reproducible.
    rows.sort(key=lambda r: r["fnv"])
    return rows


def fit(img, box):
    """Nearest-scale `img` to fit `box`x`box`, preserving aspect."""
    w, h = img.size
    if w == 0 or h == 0:
        return img
    s = min(box / w, box / h)
    nw, nh = max(1, int(w * s)), max(1, int(h * s))
    return img.resize((nw, nh), Image.NEAREST)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("render_dir")
    ap.add_argument("--cols", type=int, default=7)
    ap.add_argument("--rows", type=int, default=7)
    args = ap.parse_args()

    rows = load_manifest(args.render_dir)
    if not rows:
        print("no manifest rows", file=sys.stderr)
        return 1

    sheets_dir = os.path.join(args.render_dir, "sheets")
    os.makedirs(sheets_dir, exist_ok=True)
    try:
        font = ImageFont.truetype(
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 11
        )
        font_idx = ImageFont.truetype(
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 13
        )
    except OSError:
        font = ImageFont.load_default()
        font_idx = font

    per_sheet = args.cols * args.rows
    cell_w = CELL_IMG + 2 * PAD
    cell_h = CELL_IMG + LABEL_H + 2 * PAD
    sheet_w = args.cols * cell_w
    sheet_h = args.rows * cell_h

    index_lines = ["global_index\tfnv1a\ttier\twidth\theight\tbpp\tsheet\tcell\n"]
    n_sheets = (len(rows) + per_sheet - 1) // per_sheet

    for s in range(n_sheets):
        sheet = Image.new("RGB", (sheet_w, sheet_h), BG)
        draw = ImageDraw.Draw(sheet)
        for c in range(per_sheet):
            gi = s * per_sheet + c
            if gi >= len(rows):
                break
            r = rows[gi]
            col, row = c % args.cols, c // args.cols
            x0, y0 = col * cell_w, row * cell_h
            draw.rectangle(
                [x0 + 1, y0 + 1, x0 + cell_w - 2, y0 + cell_h - 2], fill=CELL_BG
            )
            png = os.path.join(args.render_dir, r["fnv"] + ".png")
            if os.path.exists(png):
                try:
                    im = fit(Image.open(png).convert("RGBA"), CELL_IMG)
                    ix = x0 + PAD + (CELL_IMG - im.size[0]) // 2
                    iy = y0 + PAD + (CELL_IMG - im.size[1]) // 2
                    sheet.paste(im, (ix, iy), im)
                except Exception:  # noqa: BLE001 - skip an unreadable cell
                    pass
            # Index badge (top-left, high contrast).
            badge = str(gi)
            tb = draw.textbbox((0, 0), badge, font=font_idx)
            bw, bh = tb[2] - tb[0], tb[3] - tb[1]
            draw.rectangle(
                [x0 + 2, y0 + 2, x0 + 6 + bw, y0 + 6 + bh], fill=IDX_BG
            )
            draw.text((x0 + 4, y0 + 3), badge, fill=(255, 255, 255), font=font_idx)
            # Caption: dims + tier.
            cap = f"{r['w']}x{r['h']} {r['bpp']}b {r['tier'][0]}"
            draw.text(
                (x0 + PAD, y0 + PAD + CELL_IMG + 3), cap, fill=TEXT, font=font
            )
            index_lines.append(
                f"{gi}\t{r['fnv']}\t{r['tier']}\t{r['w']}\t{r['h']}\t{r['bpp']}\t{s}\t{c}\n"
            )
        sheet.save(os.path.join(sheets_dir, f"sheet_{s:03d}.png"))

    with open(os.path.join(args.render_dir, "index.tsv"), "w") as f:
        f.writelines(index_lines)
    print(
        f"wrote {n_sheets} sheets ({per_sheet}/sheet) for {len(rows)} textures "
        f"-> {sheets_dir}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
