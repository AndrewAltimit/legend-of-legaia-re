#!/usr/bin/env python3
"""widget-draw-sweep.py - join a live frame's UI sprites to the SCUS widget table.

The battle / menu chrome is drawn out of one **widget-class table** in
`SCUS_942.54` (VA `0x800732A4`, `0x0C` stride - see
`docs/subsystems/battle.md` and `legaia_asset::ui_widgets`). Every record is a
`(u, v, w, h)` source rect on the resident system-UI sheet plus a packed
palette byte, and both SCUS draw routines copy those four bytes verbatim into
the `SPRT` packet they queue.

That makes the join trivial and exact: walk the `SPRT` packets sitting in a
save state's main RAM (libgpu leaves its ordering-table nodes there, so the RAM
image *is* the frame's display list), look each packet's rect up in the table,
and every UI sprite on screen reports which widget record drew it and at which
seat. Unmatched rects are the surfaces that do **not** come off this table -
glyphs, numerals, effect billboards.

Usage:

    widget-draw-sweep.py <state.mcr|state.sstate|ram.bin> [--scus PATH]
                         [--min-y N] [--max-y N] [--only-matched]

A `.mcr` / `.sstate` argument is unpacked through `mednafen-state extract` /
`pcsxr-state extract` (built binaries in `target/release/`); a plain `.bin` is
taken as a raw main-RAM image already.

Prints coordinates only - no Sony bytes are emitted or written.
"""

from __future__ import annotations

import argparse
import struct
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

WIDGET_TABLE_VA = 0x800732A4
WIDGET_STRIDE = 0x0C
WIDGET_COUNT = 0x9D
SPRT_CODES = {0x64, 0x65, 0x66, 0x67}
SPRT_TAG_LEN = 4  # GP0 words after the tag


def exe_offset(scus: bytes, va: int) -> int:
    if scus[:8] != b"PS-X EXE":
        raise SystemExit("not a PS-X EXE")
    t_addr, t_size = struct.unpack_from("<II", scus, 0x18)
    if not (t_addr <= va < t_addr + t_size):
        raise SystemExit(f"VA {va:#010x} is outside the text segment")
    return va - t_addr + 0x800


def clut_fb(pal: int) -> tuple[int, int]:
    """Widget palette byte -> VRAM (x, y) of its 16-entry CLUT."""
    if pal & 0x40:
        k = pal & 0x3F
        return 896 + (k & 3) * 16, 498 + (k >> 2)
    cba = 0x7FC0 + (pal & 0x7F)
    return (cba & 0x3F) * 16, (cba >> 6) & 0x1FF


def load_widgets(scus: bytes) -> dict[tuple[int, int, int, int], list[int]]:
    base = exe_offset(scus, WIDGET_TABLE_VA)
    by_rect: dict[tuple[int, int, int, int], list[int]] = {}
    for i in range(WIDGET_COUNT):
        r = scus[base + i * WIDGET_STRIDE : base + (i + 1) * WIDGET_STRIDE]
        by_rect.setdefault((r[4], r[5], r[6], r[7]), []).append(i)
    return by_rect


def widget_record(scus: bytes, index: int) -> dict:
    base = exe_offset(scus, WIDGET_TABLE_VA) + index * WIDGET_STRIDE
    r = scus[base : base + WIDGET_STRIDE]
    return {
        "class": r[0],
        "tileset": r[1],
        "chain": struct.unpack_from("<b", r, 2)[0],
        "palette": r[3],
        "bias": struct.unpack_from("<hh", r, 8),
    }


def s16(v: int) -> int:
    return v - 0x10000 if v & 0x8000 else v


def main_ram(path: Path) -> bytes:
    if path.suffix.lower() not in (".mcr", ".sstate"):
        return path.read_bytes()
    tool = "pcsxr-state" if path.suffix.lower() == ".sstate" else "mednafen-state"
    binary = REPO / "target" / "release" / tool
    if not binary.is_file():
        found = shutil.which(tool)
        if not found:
            raise SystemExit(
                f"{tool} not found - `cargo build --release` it, or pass a raw main-RAM .bin"
            )
        binary = Path(found)
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "ram.bin"
        subprocess.run(
            [str(binary), "extract", str(path),
             "--start", "0x80000000", "--end", "0x80200000", "--out", str(out)],
            check=True, stdout=subprocess.DEVNULL,
        )
        return out.read_bytes()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("state", type=Path)
    ap.add_argument("--scus", type=Path, default=REPO / "extracted" / "SCUS_942.54")
    ap.add_argument("--min-y", type=int, default=-64)
    ap.add_argument("--max-y", type=int, default=300)
    ap.add_argument("--only-matched", action="store_true",
                    help="drop sprites whose rect is not a widget record")
    args = ap.parse_args()

    if not args.scus.is_file():
        raise SystemExit(f"no SCUS at {args.scus} - extract the disc first")
    scus = args.scus.read_bytes()
    by_rect = load_widgets(scus)
    ram = main_ram(args.state)

    rows = []
    seen: set[tuple] = set()
    for o in range(0, len(ram) - 0x14, 4):
        if ram[o + 7] not in SPRT_CODES:
            continue
        if struct.unpack_from("<I", ram, o)[0] >> 24 != SPRT_TAG_LEN:
            continue
        xy = struct.unpack_from("<I", ram, o + 8)[0]
        uvc = struct.unpack_from("<I", ram, o + 0x0C)[0]
        w, h = struct.unpack_from("<HH", ram, o + 0x10)
        x, y = s16(xy & 0xFFFF), s16(xy >> 16)
        u, v, clut = uvc & 0xFF, (uvc >> 8) & 0xFF, uvc >> 16
        if not (args.min_y <= y <= args.max_y) or not (-64 <= x <= 400):
            continue
        if not (0 < w <= 320 and 0 < h <= 256):
            continue
        ids = by_rect.get((u, v, w, h), [])
        if args.only_matched and not ids:
            continue
        # The same frame is queued into two ordering tables (double buffer);
        # collapse the duplicate so the report reads as one screen.
        key = (x, y, u, v, w, h, clut)
        if key in seen:
            continue
        seen.add(key)
        rows.append((y, x, ram[o + 7], u, v, w, h, clut, ids))

    rows.sort()
    print(f"{len(rows)} unique UI sprites in the frame")
    for y, x, code, u, v, w, h, clut, ids in rows:
        cf = ((clut & 0x3F) * 16, (clut >> 6) & 0x1FF)
        tag = ""
        if ids:
            parts = []
            for i in ids:
                rec = widget_record(scus, i)
                fits = "*" if clut_fb(rec["palette"]) == cf else " "
                parts.append(f"{fits}#{i:#04x}(cls{rec['class']},chain{rec['chain']:+d})")
            tag = " ".join(parts)
        print(f"  xy=({x:4d},{y:4d}) uv=({u:3d},{v:3d}) wh=({w:3d},{h:3d}) "
              f"code={code:#04x} clut={cf} {tag}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
