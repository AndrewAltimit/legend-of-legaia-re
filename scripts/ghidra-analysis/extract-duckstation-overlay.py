#!/usr/bin/env python3
"""
Extract a slice of PSX main RAM from a Duckstation save state (.sav).

Duckstation save states use a fixed-size ASCII header ("DUCCS" magic + game
metadata) followed immediately by a zstd-compressed binary stream containing
all emulator state: VRAM, SPU RAM, main RAM, CPU registers, etc.  The RAM
window starts at a fixed offset inside the decompressed stream; we locate it
with the same anchor-string approach as extract-mednafen-overlay.py.

The default slice extracts the overlay code window 0x801C0000..0x80200000
(256 KB) where Legaia loads runtime overlays.

Usage:
    scripts/ghidra-analysis/extract-duckstation-overlay.py SAVE.sav [--out OUT.bin]
                                                     [--start 0x801C0000]
                                                     [--end   0x80200000]
                                                     [--scus  PATH]

See docs/tooling/overlay-capture.md for the full capture pipeline.
"""

import argparse
import os
import subprocess
import struct
import sys
import tempfile

ANCHORS = [
    b"---- FIELD PROGRAM -----%d",
    b"PSX TEST PROGRAM",
    b"enter main loop",
    b"main free mem%d",
    b"h:\\prot\\cdname.dat",
]

PSX_RAM_SIZE = 2 * 1024 * 1024
PSX_RAM_KSEG0 = 0x80000000
SCUS_LOAD_ADDR = 0x80010000
PSX_EXE_HEADER = 0x800
ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"
DEFAULT_OVERLAY_START = 0x801C0000
DEFAULT_OVERLAY_END = 0x80200000


def parse_addr(s: str) -> int:
    return int(s, 16) if s.lower().startswith("0x") else int(s)


def decompress_zstd(compressed: bytes) -> bytes:
    """Decompress zstd-compressed bytes using the system zstd binary."""
    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as tf:
        tf.write(compressed)
        tmp_in = tf.name
    tmp_out = tmp_in + ".dec"
    try:
        result = subprocess.run(
            ["zstd", "-d", tmp_in, "-o", tmp_out, "--force", "-q"],
            capture_output=True,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"zstd decompression failed: {result.stderr.decode()}"
            )
        with open(tmp_out, "rb") as f:
            return f.read()
    finally:
        for p in (tmp_in, tmp_out):
            try:
                os.unlink(p)
            except FileNotFoundError:
                pass


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("save", help="Path to Duckstation save state (.sav)")
    ap.add_argument("--out", help="Output path (default: /tmp/legaia_ram_<start>_<end>.bin)")
    ap.add_argument(
        "--start", type=parse_addr, default=DEFAULT_OVERLAY_START,
        help="Slice start as PSX virtual address (default: 0x801C0000)",
    )
    ap.add_argument(
        "--end", type=parse_addr, default=DEFAULT_OVERLAY_END,
        help="Slice end (exclusive) as PSX virtual address (default: 0x80200000)",
    )
    ap.add_argument(
        "--scus", default="extracted/SCUS_942.54",
        help="Path to extracted SCUS_942.54 (default: extracted/SCUS_942.54)",
    )
    args = ap.parse_args()

    if not os.path.isfile(args.save):
        print(f"error: save state not found: {args.save}", file=sys.stderr)
        return 1
    if not os.path.isfile(args.scus):
        print(f"error: SCUS_942.54 not found at {args.scus}", file=sys.stderr)
        print("       pass --scus PATH to override", file=sys.stderr)
        return 1

    raw = open(args.save, "rb").read()

    # Duckstation saves: "DUCCS" magic at offset 0.
    if raw[:5] != b"DUCCS":
        print(f"error: not a Duckstation save (magic={raw[:8]!r})", file=sys.stderr)
        return 1

    # Locate the zstd stream that follows the ASCII header.
    zstd_off = raw.find(ZSTD_MAGIC)
    if zstd_off < 0:
        print("error: zstd magic (0xFD2FB528) not found in save state", file=sys.stderr)
        return 1
    print(f"[info] zstd stream at file offset 0x{zstd_off:X}")

    print("[info] decompressing with zstd …")
    state = decompress_zstd(raw[zstd_off:])
    print(f"[info] decompressed: {len(state):,} bytes")

    # Locate main RAM by anchoring on known SCUS strings.
    scus = open(args.scus, "rb").read()
    ram_offset_in_state = None
    used_anchor = None
    for anchor in ANCHORS:
        scus_off = scus.find(anchor)
        if scus_off < PSX_EXE_HEADER:
            continue
        state_off = state.find(anchor)
        if state_off < 0:
            continue
        ram_addr = SCUS_LOAD_ADDR + (scus_off - PSX_EXE_HEADER)
        phys = ram_addr - PSX_RAM_KSEG0
        ram_offset_in_state = state_off - phys
        used_anchor = anchor
        print(f"[info] anchor: {anchor!r}")
        print(f"       SCUS file offset 0x{scus_off:X} -> RAM 0x{ram_addr:08X}")
        print(f"       found at state offset 0x{state_off:X}")
        print(f"       => main RAM starts at state offset 0x{ram_offset_in_state:08X}")
        break

    if ram_offset_in_state is None:
        print("error: no anchor found; can't locate main RAM", file=sys.stderr)
        return 1

    # Verify with a second anchor.
    for anchor in ANCHORS:
        if anchor == used_anchor:
            continue
        scus_off = scus.find(anchor)
        if scus_off < PSX_EXE_HEADER:
            continue
        state_off = state.find(anchor)
        if state_off < 0:
            continue
        ram_addr = SCUS_LOAD_ADDR + (scus_off - PSX_EXE_HEADER)
        phys = ram_addr - PSX_RAM_KSEG0
        expected = phys + ram_offset_in_state
        if state_off != expected:
            print(
                f"WARNING: anchor {anchor!r} at 0x{state_off:X}; expected 0x{expected:X}",
                file=sys.stderr,
            )
        break

    # Extract the slice.
    if args.start < PSX_RAM_KSEG0 or args.end > PSX_RAM_KSEG0 + PSX_RAM_SIZE:
        print(
            f"error: slice [0x{args.start:08X}..0x{args.end:08X}) outside main RAM",
            file=sys.stderr,
        )
        return 1
    slice_start = ram_offset_in_state + (args.start - PSX_RAM_KSEG0)
    slice_end = ram_offset_in_state + (args.end - PSX_RAM_KSEG0)
    sliced = state[slice_start:slice_end]
    if len(sliced) != args.end - args.start:
        print(
            f"error: short read ({len(sliced)} of {args.end - args.start} bytes)",
            file=sys.stderr,
        )
        return 1

    out_path = args.out or f"/tmp/legaia_ram_{args.start:08X}_{args.end:08X}.bin"
    open(out_path, "wb").write(sliced)
    print(
        f"[ok]   wrote {out_path}: {len(sliced):,} bytes "
        f"(RAM 0x{args.start:08X}..0x{args.end:08X})"
    )

    JR_RA = bytes.fromhex("0800e003")
    n_jr = sum(1 for i in range(0, len(sliced) - 3, 4) if sliced[i:i + 4] == JR_RA)
    n_sp = sum(
        1 for i in range(0, len(sliced) - 3, 4)
        if sliced[i + 2] == 0xBD and sliced[i + 3] == 0x27 and sliced[i + 1] == 0xFF
    )
    nonzero = sum(1 for b in sliced if b)
    print(
        f"[info] {nonzero:,} nonzero bytes ({100 * nonzero / len(sliced):.1f}%); "
        f"{n_jr} `jr $ra`; {n_sp} SP prologues"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
