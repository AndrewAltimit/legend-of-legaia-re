#!/usr/bin/env python3
"""Render a battle-form character (PROT 1204) in its TRUE palette.

Offline validation of the battle-CLUT reading: the 1204 atlas image + the party
palette resident at the mesh's NOMINAL CBA rows (490..497) produce a correctly-
coloured character (blue-haired Vahn, etc.), which the bundled 1204 CLUT (the
Baka Fighter palette) does NOT. The battle character renderer uses the nominal
CBA directly - there is NO texpage->row relocation for characters (see
`docs/formats/character-mesh.md` -> Battle palette).

The palette is read from a CLEAN battle mednafen save state's VRAM (command-menu
or Begin-menu, no effect animation). Mid-battle captures overwrite the character
CLUT rows and read back garbage. Save-state VRAM is Sony data and stays local;
this script writes a local PNG only (never commit the output).

Inputs:
  --pack    extracted/PROT/1204_other5.BIN
  --save    a clean-battle mednafen save (VRAM source)
  --slot    0=Vahn 1=Noa 2=Gala
  --out     output PNG (local only)

It shells out to the built `tmd` and `mednafen-state` binaries.

Usage:
  python3 scripts/asset-investigation/render_battle_char_true.py --pack extracted/PROT/1204_other5.BIN \
      --save "$HOME/.mednafen/mcs/...mc7" --slot 0 --out /tmp/vahn_true.png
"""
import argparse, struct, re, subprocess, sys, tempfile, os
from PIL import Image

ATLAS_BASE = 0x025804
ATLAS_STRIDE = 0x8224


def run(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pack", required=True)
    ap.add_argument("--save", required=True)
    ap.add_argument("--slot", type=int, required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--tmd-bin", default="./target/release/tmd")
    ap.add_argument("--mstate-bin", default="./target/release/mednafen-state")
    args = ap.parse_args()

    pack = open(args.pack, "rb").read()
    # atlas image offsets keyed by texpage
    atlas_imgoff = {}
    for i in range(7):
        off = ATLAS_BASE + i * ATLAS_STRIDE
        co = off + 8
        _cb, _cx, _cy, cw, ch = struct.unpack_from("<IHHHH", pack, co)
        io = co + 12 + cw * ch * 2
        _ib, ix, iy, _iw, _ih = struct.unpack_from("<IHHHH", pack, io)
        atlas_imgoff[(ix >> 6) + (iy >> 8) * 16] = io + 12

    with tempfile.TemporaryDirectory() as td:
        tmd_path = os.path.join(td, "slot.tmd")
        run("./target/release/asset", "battle-char-pack", args.pack,
            "--slot", str(args.slot), "--out-tmd", tmd_path)
        prims = run(args.tmd_bin, "prims", tmd_path)
        vram_bin = os.path.join(td, "vram.bin")
        run(args.mstate_bin, "extract", args.save,
            "--start", "0x80000000", "--end", "0x80200000", "--out", vram_bin)
        # VRAM via vram-dump (BGR555) is cleaner; use it:
        run(args.mstate_bin, "vram-dump", args.save, "--out-bin", vram_bin)
        vram = open(vram_bin, "rb").read()

    def color(row, col):
        w = struct.unpack_from("<H", vram, row * 2048 + col * 2)[0]
        return ((w & 0x1f) << 3, ((w >> 5) & 0x1f) << 3, ((w >> 10) & 0x1f) << 3)

    prim_re = re.compile(
        r"cba=0x([0-9A-Fa-f]+)@\((\d+),(\d+)\).*?tsb=0x[0-9A-Fa-f]+@\((\d+),(\d+)\).*?uvs=\[(.*?)\]")
    img = Image.new("RGB", (256, 256), (15, 15, 25))
    px = img.load()
    for line in prims.splitlines():
        m = prim_re.search(line)
        if not m:
            continue
        cba = int(m.group(1), 16)
        cba_x = (cba & 0x3f) * 16
        tx, ty = int(m.group(4)), int(m.group(5))
        tp = (tx >> 6) + (ty >> 8) * 16
        if tp not in atlas_imgoff:
            continue
        row = (cba >> 6) & 0x1ff  # nominal CBA row (no texpage relocation)
        uvs = re.findall(r"\((\d+),\s*(\d+)\)", m.group(6))
        if len(uvs) < 3:
            continue
        pts = [(int(a), int(b)) for a, b in uvs[:3]]
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        minx, maxx = max(0, min(xs)), min(255, max(xs))
        miny, maxy = max(0, min(ys)), min(255, max(ys))
        (x0, y0), (x1, y1), (x2, y2) = pts
        den = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2)
        if den == 0:
            continue
        base = atlas_imgoff[tp]
        for yy in range(miny, maxy + 1):
            for xx in range(minx, maxx + 1):
                a = ((y1 - y2) * (xx - x2) + (x2 - x1) * (yy - y2)) / den
                b = ((y2 - y0) * (xx - x2) + (x0 - x2) * (yy - y2)) / den
                if a < -0.01 or b < -0.01 or (1 - a - b) < -0.01:
                    continue
                byte = pack[base + yy * 128 + (xx >> 1)]
                idx = (byte & 0xf) if (xx & 1) == 0 else (byte >> 4)
                px[xx, yy] = color(row, cba_x + idx)
    img.save(args.out)
    print(f"wrote {args.out} (nominal CBA rows: Vahn 490/491, Noa 492/493, "
          f"Gala 494/495 - sampled from the clean-battle save's VRAM)")


if __name__ == "__main__":
    sys.exit(main())
