#!/usr/bin/env python3
"""Find the on-disc PROT entry that carries the dialog-font glyph bitmap.

Reads the raw 4bpp VRAM bytes the font extractor wrote to
`extracted/font/dialog_font_vram_4bpp.bin` and searches every PROT entry
for a long enough match. The font tile-page is 32 KB; most cells contain
short non-zero spans surrounded by zeros so simple grep-style searches
pick patterns the font shares with random TIM data.

This script tries three search strategies in order:

1. **Direct slice match** - original strategy. Picks 64-byte slices from
   regions known to carry glyph data.
2. **Glyph-row signature match** - extract the 8-byte row signatures
   for each character cell, search every PROT entry for a long run of
   consecutive matching rows. Survives any byte-level permutation that
   keeps the row layout intact.
3. **LZS-decompressed match** - for entries the static categorizer marks
   as `LzsContainer` or `Pochi*` filler (most common font carriers in
   the disc layout), LZS-decompress and re-run the slice search against
   the decoded bytes. Requires the `lzs-decode` binary on `PATH` (built
   via `cargo build -p legaia-lzs --release`).

Run from the repo root:
    python3 scripts/asset-investigation/find-font-carrier.py
"""
from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
EXTRACTED = ROOT / "extracted"
FONT_BIN = EXTRACTED / "font" / "dialog_font_vram_4bpp.bin"
PROT_DIR = EXTRACTED / "PROT"


def slice_probes(font_bytes: bytes) -> list[tuple[int, bytes]]:
    """Build slice-match probes - fixed offsets, fixed length."""
    candidates = [
        (0x600, 64),
        (0x1000, 64),
        (0x2000, 64),
        (0x4000, 64),
    ]
    out: list[tuple[int, bytes]] = []
    for off, n in candidates:
        slice_bytes = font_bytes[off : off + n]
        if any(slice_bytes):
            out.append((off, slice_bytes))
    return out


def glyph_row_probes(font_bytes: bytes) -> list[tuple[int, bytes]]:
    """Build glyph-row signature probes.

    The font is a 256x256 pixel 4bpp tile-page = 128 bytes per row of
    pixels. Each glyph cell is 16x16 pixels → 8 bytes wide × 16 rows tall.
    For one heavy-data row of a heavy-data glyph (say 'M' at column 13,
    row 4) we read 16 rows × 8 bytes = 128 bytes that span the cell.
    Any concatenation of >= 2 such cells gives a long unique signature
    that direct-slice probes miss when the tile-page is stored row-major.

    Cells are arranged as 16 cols × 14 rows, with the glyph cells
    starting at character byte 0x20 (so cell index `c` corresponds to
    char byte `c+0x20` in the dialog stream). We pick a row of 4 cells
    that all carry heavy data: 'M' (0x4D), 'N' (0x4E), 'O' (0x4F),
    'P' (0x50) - middle-of-row capital letters.
    """
    PAGE_BYTES_PER_ROW = 128
    CELL_BYTES_W = 8
    CELL_PX_H = 16
    out: list[tuple[int, bytes]] = []
    for first_char, count in [(0x41, 8), (0x4D, 4), (0x61, 8)]:
        sig = bytearray()
        for col_offset in range(count):
            c = first_char + col_offset
            cell_col = c & 0x0F
            cell_row = (c >> 4) - 0x02
            base_x = cell_col * CELL_BYTES_W
            base_y = cell_row * CELL_PX_H
            for r in range(CELL_PX_H):
                row_off = (base_y + r) * PAGE_BYTES_PER_ROW + base_x
                if row_off + CELL_BYTES_W > len(font_bytes):
                    break
                sig.extend(font_bytes[row_off : row_off + CELL_BYTES_W])
        if any(sig):
            out.append((first_char, bytes(sig)))
    return out


def search_buffer(buf: bytes, probes: list[tuple[int, bytes]]) -> list[tuple[int, int]]:
    """Return list of `(probe_label, offset_in_buf)` for each probe hit."""
    hits: list[tuple[int, int]] = []
    for label, needle in probes:
        idx = buf.find(needle)
        if idx >= 0:
            hits.append((label, idx))
    return hits


def lzs_decode_to(buf: bytes, work_dir: Path) -> list[bytes] | None:
    """Try LZS-decoding `buf` via the `lzs-decode container` subcommand.
    Returns a list of decompressed section payloads on success, None
    otherwise (most PROT entries aren't LZS containers).
    """
    binary = shutil.which("lzs-decode")
    if not binary:
        return None
    src = work_dir / "in.bin"
    src.write_bytes(buf)
    out_dir = work_dir / "sections"
    if out_dir.exists():
        for f in out_dir.iterdir():
            f.unlink()
    out_dir.mkdir(parents=True, exist_ok=True)
    try:
        subprocess.run(
            [binary, "container", str(src), str(out_dir)],
            check=True,
            capture_output=True,
            timeout=10,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return None
    finally:
        src.unlink(missing_ok=True)
    sections = []
    for path in sorted(out_dir.glob("*")):
        sections.append(path.read_bytes())
        path.unlink()
    return sections or None


def main() -> int:
    if not FONT_BIN.exists():
        sys.exit(f"missing {FONT_BIN} - run `font-extract` first")
    if not PROT_DIR.exists():
        sys.exit(f"missing {PROT_DIR} - run `legaia-extract` first")

    font_bytes = FONT_BIN.read_bytes()

    slices = slice_probes(font_bytes)
    rows = glyph_row_probes(font_bytes)
    print(f"slice probes: {len(slices)}, row-signature probes: {len(rows)}")

    paths = sorted(PROT_DIR.glob("*.BIN"))
    print(f"searching {len(paths)} PROT entries (raw)...")
    raw_hits: dict[str, list[tuple[str, int]]] = {}
    for path in paths:
        data = path.read_bytes()
        hits = search_buffer(data, [(f"slice@{o:x}", n) for o, n in slices])
        hits += search_buffer(data, [(f"rows@{o:x}", n) for o, n in rows])
        if hits:
            raw_hits[path.name] = hits

    if raw_hits:
        print(f"\nraw hits: {len(raw_hits)} entries")
        for name, hs in sorted(raw_hits.items()):
            print(f"  {name}: {hs}")
    else:
        print("no raw hits.")

    # LZS pass - best-effort, requires lzs-decode on PATH.
    if not shutil.which("lzs-decode"):
        print("\nskipping LZS pass: `lzs-decode` not on PATH (build via `cargo build -p legaia-lzs --release` and add target/release to PATH)")
        return 0 if raw_hits else 1

    print("\nLZS pass on candidate entries...")
    lzs_hits: dict[str, list[tuple[str, int, int]]] = {}
    with tempfile.TemporaryDirectory() as td:
        td_path = Path(td)
        decoded = 0
        for path in paths:
            data = path.read_bytes()
            if len(data) < 16:
                continue
            sections = lzs_decode_to(data, td_path)
            if sections is None:
                continue
            decoded += 1
            for sec_idx, decoded_bytes in enumerate(sections):
                hits = search_buffer(
                    decoded_bytes, [(f"slice@{o:x}", n) for o, n in slices]
                )
                hits += search_buffer(
                    decoded_bytes, [(f"rows@{o:x}", n) for o, n in rows]
                )
                for label, off in hits:
                    lzs_hits.setdefault(path.name, []).append((label, sec_idx, off))
        print(f"LZS-decoded {decoded} entries")
        if lzs_hits:
            print(f"LZS hits: {len(lzs_hits)} entries")
            for name, hs in sorted(lzs_hits.items()):
                print(f"  {name}: {hs}")
        else:
            print("no LZS hits.")

    if raw_hits or lzs_hits:
        return 0
    print(
        "\nno match in either pass - font is likely uploaded by an overlay-resident\n"
        "routine that copies from a buffer the static analysis hasn't classified yet.\n"
        "See docs/formats/dialog-font.md for the trace path."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
